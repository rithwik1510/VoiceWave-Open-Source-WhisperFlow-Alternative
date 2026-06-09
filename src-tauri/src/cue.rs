//! Low-latency hotkey cue playback.
//!
//! How polished dictation apps (Wispr Flow et al.) make cues feel instant:
//!
//! 1. The sound triggers on the *physical input action* (hotkey press /
//!    release), not on downstream state transitions that lag behind it.
//!    Callers here are `start_dictation` / `stop_dictation` entry points.
//! 2. Playback goes through a pre-warmed, low-latency output stream that
//!    *mixes* voices. `PlaySoundW` (the previous approach) buffers 100-300 ms
//!    through the legacy MME path and cancels the playing sound when a new
//!    one starts — which produced both the perceived lag and the cut-offs.
//!
//! This module keeps one cpal (WASAPI shared-mode) output stream alive on a
//! dedicated thread; each cue is mixed additively, so a fast press/release
//! overlaps the two sounds naturally instead of truncating.

use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, OnceLock};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CueSound {
    /// Hotkey pressed — dictation is starting.
    Open,
    /// Hotkey released / dictation ended.
    Close,
}

static CUE_OPEN_WAV: &[u8] = include_bytes!("../assets/cue_open.wav");
static CUE_CLOSE_WAV: &[u8] = include_bytes!("../assets/cue_close.wav");

/// Per-voice mix gain. Two cues overlapping at 0.55 peak each could clip;
/// 0.85 keeps the sum inside [-1, 1] for all realistic overlaps.
const VOICE_GAIN: f32 = 0.85;

struct ActiveVoice {
    samples: Arc<Vec<f32>>,
    position: usize,
}

static CUE_SENDER: OnceLock<Option<Sender<CueSound>>> = OnceLock::new();

fn cues_enabled() -> bool {
    match std::env::var("VOICEWAVE_CUE_SOUNDS") {
        Ok(value) => !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "off"
        ),
        Err(_) => true,
    }
}

/// Open the output stream ahead of the first cue so the first hotkey press
/// doesn't pay the device-open cost. Call once at app startup.
pub fn prewarm() {
    let _ = sender();
}

pub fn play(cue: CueSound) {
    if !cues_enabled() {
        return;
    }
    if let Some(tx) = sender() {
        let _ = tx.send(cue);
    }
}

fn sender() -> Option<&'static Sender<CueSound>> {
    CUE_SENDER
        .get_or_init(|| {
            let (tx, rx) = mpsc::channel::<CueSound>();
            std::thread::Builder::new()
                .name("voicewave-cue-audio".to_string())
                .spawn(move || audio_thread(rx))
                .ok()?;
            Some(tx)
        })
        .as_ref()
}

fn audio_thread(rx: mpsc::Receiver<CueSound>) {
    let host = cpal::default_host();
    let Some(device) = host.default_output_device() else {
        eprintln!("voicewave: cue playback disabled (no default output device)");
        return;
    };
    let Ok(supported) = device.default_output_config() else {
        eprintln!("voicewave: cue playback disabled (no default output config)");
        return;
    };
    let sample_rate = supported.sample_rate().0;
    let channels = supported.channels() as usize;
    let sample_format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();

    let open_samples = Arc::new(decode_wav_to_rate(CUE_OPEN_WAV, sample_rate));
    let close_samples = Arc::new(decode_wav_to_rate(CUE_CLOSE_WAV, sample_rate));

    let voices: Arc<Mutex<Vec<ActiveVoice>>> = Arc::new(Mutex::new(Vec::new()));
    let voices_for_callback = voices.clone();

    let mix_frame = move |voices: &mut Vec<ActiveVoice>| -> f32 {
        let mut sum = 0.0_f32;
        for voice in voices.iter_mut() {
            if let Some(sample) = voice.samples.get(voice.position) {
                sum += sample * VOICE_GAIN;
                voice.position += 1;
            }
        }
        voices.retain(|voice| voice.position < voice.samples.len());
        sum.clamp(-1.0, 1.0)
    };

    let err_fn = |err| eprintln!("voicewave: cue output stream error: {err}");
    let stream = match sample_format {
        cpal::SampleFormat::F32 => device.build_output_stream(
            &config,
            move |data: &mut [f32], _| {
                let mut voices = match voices_for_callback.lock() {
                    Ok(guard) => guard,
                    Err(_) => return,
                };
                for frame in data.chunks_mut(channels) {
                    let value = mix_frame(&mut voices);
                    for channel_sample in frame {
                        *channel_sample = value;
                    }
                }
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_output_stream(
            &config,
            move |data: &mut [i16], _| {
                let mut voices = match voices_for_callback.lock() {
                    Ok(guard) => guard,
                    Err(_) => return,
                };
                for frame in data.chunks_mut(channels) {
                    let value = (mix_frame(&mut voices) * i16::MAX as f32) as i16;
                    for channel_sample in frame {
                        *channel_sample = value;
                    }
                }
            },
            err_fn,
            None,
        ),
        other => {
            eprintln!("voicewave: cue playback disabled (unsupported sample format {other:?})");
            return;
        }
    };

    let stream = match stream {
        Ok(stream) => stream,
        Err(err) => {
            eprintln!("voicewave: cue playback disabled (stream build failed: {err})");
            return;
        }
    };
    if let Err(err) = stream.play() {
        eprintln!("voicewave: cue playback disabled (stream start failed: {err})");
        return;
    }

    while let Ok(cue) = rx.recv() {
        let samples = match cue {
            CueSound::Open => open_samples.clone(),
            CueSound::Close => close_samples.clone(),
        };
        if let Ok(mut voices) = voices.lock() {
            voices.push(ActiveVoice {
                samples,
                position: 0,
            });
        }
    }
}

/// Minimal RIFF/WAV reader for the embedded cues (PCM16 mono, generated by
/// the asset pipeline), resampled to the output device rate.
fn decode_wav_to_rate(bytes: &[u8], target_rate: u32) -> Vec<f32> {
    let Some(data) = wav_data_chunk(bytes) else {
        return Vec::new();
    };
    let source_rate = wav_sample_rate(bytes).unwrap_or(44_100);
    let samples: Vec<f32> = data
        .chunks_exact(2)
        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]) as f32 / 32_768.0)
        .collect();
    if source_rate == target_rate {
        return samples;
    }
    crate::audio::resample_linear(&samples, source_rate, target_rate)
}

fn wav_sample_rate(bytes: &[u8]) -> Option<u32> {
    let fmt = find_chunk(bytes, b"fmt ")?;
    fmt.get(4..8)
        .map(|raw| u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

fn wav_data_chunk(bytes: &[u8]) -> Option<&[u8]> {
    find_chunk(bytes, b"data")
}

fn find_chunk<'a>(bytes: &'a [u8], id: &[u8; 4]) -> Option<&'a [u8]> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let mut offset = 12;
    while offset + 8 <= bytes.len() {
        let chunk_id = &bytes[offset..offset + 4];
        let size = u32::from_le_bytes([
            bytes[offset + 4],
            bytes[offset + 5],
            bytes[offset + 6],
            bytes[offset + 7],
        ]) as usize;
        let body_start = offset + 8;
        let body_end = (body_start + size).min(bytes.len());
        if chunk_id == id {
            return Some(&bytes[body_start..body_end]);
        }
        // chunks are word-aligned
        offset = body_start + size + (size % 2);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{find_chunk, wav_sample_rate, CUE_CLOSE_WAV, CUE_OPEN_WAV};

    #[test]
    fn embedded_cues_are_valid_pcm16_wavs() {
        for (name, bytes) in [("open", CUE_OPEN_WAV), ("close", CUE_CLOSE_WAV)] {
            let data = find_chunk(bytes, b"data")
                .unwrap_or_else(|| panic!("{name} cue must contain a data chunk"));
            assert!(
                !data.is_empty() && data.len() % 2 == 0,
                "{name} cue data chunk must be non-empty PCM16"
            );
            assert_eq!(
                wav_sample_rate(bytes),
                Some(44_100),
                "{name} cue must be 44.1 kHz"
            );
        }
    }
}
