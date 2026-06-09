use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    SampleFormat, Stream, StreamConfig,
};
use serde::{Deserialize, Serialize};
use std::{
    sync::mpsc::{self, Receiver, RecvTimeoutError, Sender},
    time::{Duration, Instant},
};

pub const TARGET_SAMPLE_RATE: u32 = 16_000;
const FRAME_SIZE: usize = 320;
const STOP_SIGNAL_DEBOUNCE_MS: u64 = 160;
// Minimum wait after the user releases the hotkey before we allow the
// capture loop to break on silence. This IS the effective post-release
// capture window for soft word endings ("s", "th", "f", drifted "d")
// that trail below the RMS VAD threshold and therefore never reset
// `post_release_last_voiced_at`. Must stay >= the 300 ms tail pad in
// trim_capture_edges (and trim_low_energy_edges in
// inference/audio_pipeline.rs); otherwise neither pad has samples to
// work with and the user sees the last 2-3 words clipped.
const RELEASE_TAIL_MIN_WAIT_MS: u64 = 300;
const RELEASE_TAIL_SILENCE_CONFIRM_MS: u64 = 60;
// Fast tail exit for the common case: the user finished speaking, paused,
// then released the hotkey. If the last voiced frame is at least
// PRE_RELEASE_SILENCE_PAD_MS before release, the 300 ms soft-consonant pad
// that RELEASE_TAIL_MIN_WAIT_MS exists to protect is already present inside
// the captured audio, and waiting the full minimum is pure dead time. The
// 350 ms requirement = the 300 ms trim pad plus margin for sub-threshold
// sound that can trail past the last frame the RMS VAD counted as voiced.
const PRE_RELEASE_SILENCE_PAD_MS: u64 = 350;
const RELEASE_TAIL_FAST_EXIT_MS: u64 = 120;
const VAD_PRE_ROLL_FRAMES: usize = 4;

#[derive(Debug, Clone)]
pub struct AudioFrame {
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct AudioCaptureService {
    pub target_sample_rate: u32,
}

pub struct MicLevelStream {
    pub stream: Stream,
    pub level_rx: Receiver<f32>,
    pub error_rx: Receiver<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AudioQualityBand {
    Good,
    Fair,
    Poor,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioQualityReport {
    pub sample_rate: u32,
    pub segment_count: u32,
    pub total_samples: u64,
    pub duration_ms: u64,
    pub rms: f32,
    pub peak: f32,
    pub clipping_ratio: f32,
    pub low_energy_frame_ratio: f32,
    pub estimated_snr_db: f32,
    pub quality: AudioQualityBand,
    pub issues: Vec<String>,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct CaptureOptions {
    pub vad_config: VadConfig,
    pub max_capture_duration: Duration,
    pub silence_timeout: Duration,
    pub release_tail: Duration,
    pub preserve_full_capture: bool,
    /// During the first `vad_warmup_duration` of capture, use this lower
    /// threshold instead of `vad_config.threshold`. Prevents the system
    /// from bailing out on quiet speakers before they've had a chance to
    /// produce detectable audio. Set to None to disable warmup.
    pub vad_warmup_threshold: Option<f32>,
    pub vad_warmup_duration: Duration,
}

impl Default for CaptureOptions {
    fn default() -> Self {
        Self {
            vad_config: VadConfig::default(),
            max_capture_duration: Duration::from_secs(12),
            silence_timeout: Duration::from_millis(750),
            release_tail: Duration::from_millis(0),
            preserve_full_capture: false,
            vad_warmup_threshold: None,
            vad_warmup_duration: Duration::from_millis(0),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("failed to query input devices: {0}")]
    Devices(cpal::DevicesError),
    #[error("no input device is available")]
    MissingInputDevice,
    #[error("requested input device '{0}' was not found")]
    DeviceNotFound(String),
    #[error("failed to query default input config: {0}")]
    DefaultInputConfig(cpal::DefaultStreamConfigError),
    #[error("failed to build input stream: {0}")]
    BuildStream(cpal::BuildStreamError),
    #[error("failed to start input stream: {0}")]
    PlayStream(cpal::PlayStreamError),
    #[error("unsupported sample format: {0}")]
    UnsupportedSampleFormat(String),
    #[error("audio stream runtime error: {0}")]
    RuntimeStream(String),
    #[error("no speech detected; try lowering VAD threshold or speaking closer to the mic")]
    NoSpeechDetected,
    #[error("audio capture cancelled")]
    Cancelled,
}

impl Default for AudioCaptureService {
    fn default() -> Self {
        Self {
            target_sample_rate: TARGET_SAMPLE_RATE,
        }
    }
}

impl AudioCaptureService {
    pub fn list_input_devices(&self) -> Vec<String> {
        let host = cpal::default_host();
        let Ok(devices) = host.input_devices() else {
            return Vec::new();
        };
        devices.filter_map(|d| d.name().ok()).collect()
    }

    pub fn normalize_frame(&self, frame: AudioFrame) -> Vec<f32> {
        let mono = downmix_to_mono(&frame.samples, frame.channels);
        resample_linear(&mono, frame.sample_rate, self.target_sample_rate)
    }

    pub fn capture_segments_from_microphone(
        &self,
        requested_device_name: Option<&str>,
        options: CaptureOptions,
    ) -> Result<Vec<Vec<f32>>, AudioError> {
        self.capture_segments_from_microphone_with_cancel(requested_device_name, options, || false)
    }

    pub fn capture_segments_from_microphone_with_cancel<F>(
        &self,
        requested_device_name: Option<&str>,
        options: CaptureOptions,
        should_cancel: F,
    ) -> Result<Vec<Vec<f32>>, AudioError>
    where
        F: Fn() -> bool,
    {
        self.capture_segments_from_microphone_with_signals(
            requested_device_name,
            options,
            should_cancel,
            || false,
        )
    }

    pub fn capture_segments_from_microphone_with_signals<F, G>(
        &self,
        requested_device_name: Option<&str>,
        options: CaptureOptions,
        should_cancel: F,
        should_stop: G,
    ) -> Result<Vec<Vec<f32>>, AudioError>
    where
        F: Fn() -> bool,
        G: Fn() -> bool,
    {
        self.capture_segments_from_microphone_with_signals_and_observer(
            requested_device_name,
            options,
            should_cancel,
            should_stop,
            || {},
            |_normalized_chunk, _voiced_chunk| {},
        )
    }

    pub fn capture_segments_from_microphone_with_signals_and_observer<F, G, H, I>(
        &self,
        requested_device_name: Option<&str>,
        options: CaptureOptions,
        should_cancel: F,
        should_stop: G,
        mut on_stream_ready: I,
        mut on_normalized_chunk: H,
    ) -> Result<Vec<Vec<f32>>, AudioError>
    where
        F: Fn() -> bool,
        G: Fn() -> bool,
        I: FnMut(),
        H: FnMut(&[f32], bool),
    {
        let host = cpal::default_host();
        let device = self.select_input_device(&host, requested_device_name)?;
        let supported_config = device
            .default_input_config()
            .map_err(AudioError::DefaultInputConfig)?;

        let sample_rate = supported_config.sample_rate().0;
        let channels = supported_config.channels();
        let stream_config: StreamConfig = supported_config.clone().into();
        let sample_format = supported_config.sample_format();

        let (audio_tx, audio_rx) = mpsc::channel::<Vec<f32>>();
        let (error_tx, error_rx) = mpsc::channel::<String>();

        let stream =
            build_input_stream(&device, &stream_config, sample_format, audio_tx, error_tx)?;
        stream.play().map_err(AudioError::PlayStream)?;
        on_stream_ready();

        let segments = self.collect_segments_from_stream(
            sample_rate,
            channels,
            audio_rx,
            error_rx,
            options,
            should_cancel,
            should_stop,
            &mut on_normalized_chunk,
        )?;

        drop(stream);
        if segments.is_empty() {
            return Err(AudioError::NoSpeechDetected);
        }
        Ok(segments)
    }

    pub fn probe_input_device(
        &self,
        requested_device_name: Option<&str>,
    ) -> Result<(), AudioError> {
        let host = cpal::default_host();
        let device = self.select_input_device(&host, requested_device_name)?;
        let _ = device
            .default_input_config()
            .map_err(AudioError::DefaultInputConfig)?;
        Ok(())
    }

    pub fn start_level_monitor(
        &self,
        requested_device_name: Option<&str>,
    ) -> Result<MicLevelStream, AudioError> {
        let host = cpal::default_host();
        let device = self.select_input_device(&host, requested_device_name)?;
        let supported_config = device
            .default_input_config()
            .map_err(AudioError::DefaultInputConfig)?;

        let stream_config: StreamConfig = supported_config.clone().into();
        let sample_format = supported_config.sample_format();

        let (level_tx, level_rx) = mpsc::channel::<f32>();
        let (error_tx, error_rx) = mpsc::channel::<String>();

        let stream =
            build_input_level_stream(&device, &stream_config, sample_format, level_tx, error_tx)?;
        stream.play().map_err(AudioError::PlayStream)?;

        Ok(MicLevelStream {
            stream,
            level_rx,
            error_rx,
        })
    }

    fn select_input_device(
        &self,
        host: &cpal::Host,
        requested_device_name: Option<&str>,
    ) -> Result<cpal::Device, AudioError> {
        if let Some(name) = requested_device_name {
            let devices = host.input_devices().map_err(AudioError::Devices)?;
            for device in devices {
                if let Ok(device_name) = device.name() {
                    if device_name == name {
                        return Ok(device);
                    }
                }
            }
            if let Some(default_device) = host.default_input_device() {
                let default_name = default_device.name().unwrap_or_default();
                if is_likely_low_quality_asr_input(&default_name) {
                    let devices = host.input_devices().map_err(AudioError::Devices)?;
                    for device in devices {
                        if let Ok(device_name) = device.name() {
                            if !is_likely_low_quality_asr_input(&device_name) {
                                eprintln!(
                                    "voicewave: requested input '{}' missing; using higher-quality fallback '{}' instead of low-quality default '{}'",
                                    name, device_name, default_name
                                );
                                return Ok(device);
                            }
                        }
                    }
                }
                eprintln!(
                    "voicewave: requested input device '{}' not found; falling back to default input device",
                    name
                );
                return Ok(default_device);
            }
            return Err(AudioError::DeviceNotFound(name.to_string()));
        }

        host.default_input_device()
            .ok_or(AudioError::MissingInputDevice)
    }

    fn collect_segments_from_stream(
        &self,
        sample_rate: u32,
        channels: u16,
        audio_rx: Receiver<Vec<f32>>,
        error_rx: Receiver<String>,
        options: CaptureOptions,
        should_cancel: impl Fn() -> bool,
        should_stop: impl Fn() -> bool,
        mut on_normalized_chunk: impl FnMut(&[f32], bool),
    ) -> Result<Vec<Vec<f32>>, AudioError> {
        let started = Instant::now();
        let mut vad = VadSegmenter::new(options.vad_config);
        let mut normalized_pending = Vec::new();
        let mut segments = Vec::new();
        let mut capture_accum = Vec::new();
        let mut last_voice_at: Option<Instant> = None;
        let mut stop_requested_at: Option<Instant> = None;
        let mut released_after_silence = false;
        let mut post_release_last_voiced_at: Option<Instant> = None;
        let mut voiced_frame_count = 0usize;
        let mut speech_detected = false;

        while started.elapsed() <= options.max_capture_duration {
            if should_cancel() {
                return Err(AudioError::Cancelled);
            }

            if stop_requested_at.is_none() && should_stop() {
                if started.elapsed() < Duration::from_millis(STOP_SIGNAL_DEBOUNCE_MS) {
                    continue;
                }
                stop_requested_at = Some(Instant::now());
                released_after_silence = last_voice_at.is_none_or(|at| {
                    at.elapsed() >= Duration::from_millis(PRE_RELEASE_SILENCE_PAD_MS)
                });
                if options.release_tail.is_zero() {
                    break;
                }
            }
            if let Some(released_at) = stop_requested_at {
                let elapsed = released_at.elapsed();
                if elapsed >= options.release_tail {
                    break;
                }
                // Fast path only while NOTHING voiced has arrived after the
                // release; any post-release speech falls back to the full
                // minimum wait so resumed words keep their tail pad.
                let fast_exit_ok =
                    released_after_silence && post_release_last_voiced_at.is_none();
                let min_wait_ms = if fast_exit_ok {
                    RELEASE_TAIL_FAST_EXIT_MS
                } else {
                    RELEASE_TAIL_MIN_WAIT_MS
                }
                .min(options.release_tail.as_millis() as u64);
                let min_wait = Duration::from_millis(min_wait_ms);
                if elapsed >= min_wait {
                    let recent_post_release_voice = post_release_last_voiced_at.is_some_and(|at| {
                        at.elapsed() <= Duration::from_millis(RELEASE_TAIL_SILENCE_CONFIRM_MS)
                    });
                    if !recent_post_release_voice {
                        break;
                    }
                }
            }

            if let Ok(stream_err) = error_rx.try_recv() {
                return Err(AudioError::RuntimeStream(stream_err));
            }

            match audio_rx.recv_timeout(Duration::from_millis(10)) {
                Ok(raw_chunk) => {
                    let normalized = self.normalize_frame(AudioFrame {
                        sample_rate,
                        channels,
                        samples: raw_chunk,
                    });
                    let effective_threshold = match options.vad_warmup_threshold {
                        Some(warmup) if started.elapsed() < options.vad_warmup_duration => warmup,
                        _ => options.vad_config.threshold,
                    };
                    let chunk_is_voiced = rms(&normalized) >= effective_threshold;
                    if stop_requested_at.is_some() && chunk_is_voiced {
                        post_release_last_voiced_at = Some(Instant::now());
                    }
                    on_normalized_chunk(&normalized, chunk_is_voiced);
                    capture_accum.extend_from_slice(&normalized);
                    normalized_pending.extend(normalized);

                    while normalized_pending.len() >= FRAME_SIZE {
                        let frame: Vec<f32> = normalized_pending.drain(..FRAME_SIZE).collect();
                        if rms(&frame) >= effective_threshold {
                            last_voice_at = Some(Instant::now());
                            voiced_frame_count = voiced_frame_count.saturating_add(1);
                            if voiced_frame_count >= options.vad_config.min_speech_frames {
                                speech_detected = true;
                            }
                        }

                        if let Some(segment) = vad.push_frame(&frame) {
                            segments.push(segment);
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    if stop_requested_at.is_some() {
                        continue;
                    }
                    if !speech_detected {
                        if started.elapsed() >= options.silence_timeout {
                            break;
                        }
                        continue;
                    }

                    // For push-to-talk flows (release_tail > 0), never auto-finish
                    // once speech is heard; wait for explicit stop/release.
                    if options.release_tail.is_zero() {
                        if let Some(last_voice_at) = last_voice_at {
                            if last_voice_at.elapsed() >= options.silence_timeout {
                                break;
                            }
                        }
                    }
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }

        if let Some(segment) = vad.flush() {
            if !segment.is_empty() {
                segments.push(segment);
            }
        }
        if !speech_detected {
            return Ok(Vec::new());
        }

        if options.preserve_full_capture && !capture_accum.is_empty() {
            // Volume-adaptive: a fixed 0.0055 threshold was catastrophic
            // for quiet speakers whose soft syllables fell below it and
            // got trimmed as silence. See adaptive_preserve_threshold.
            let preserve_threshold =
                adaptive_preserve_threshold(&capture_accum, options.vad_config.threshold);
            let trimmed = trim_capture_edges(&capture_accum, FRAME_SIZE, preserve_threshold);
            if !trimmed.is_empty() {
                return Ok(vec![trimmed]);
            }
            return Ok(vec![capture_accum]);
        }

        if segments.is_empty() && !capture_accum.is_empty() {
            let fallback_len = (self.target_sample_rate as usize * 3).min(capture_accum.len());
            if fallback_len > 0 {
                let start = capture_accum.len() - fallback_len;
                let slice = &capture_accum[start..];
                let fallback_threshold = options.vad_config.threshold * 0.5;
                if rms(slice) >= fallback_threshold {
                    segments.push(slice.to_vec());
                }
            }
        }

        Ok(segments)
    }
}

fn is_likely_low_quality_asr_input(device_name: &str) -> bool {
    let normalized = device_name.to_ascii_lowercase();
    normalized.contains("hands-free")
        || normalized.contains("hand free")
        || normalized.contains("bluetooth headset")
        || normalized.contains("headset")
        || normalized.contains("hfp")
        || normalized.contains("ag audio")
        || normalized.contains("sco")
}

fn build_input_stream(
    device: &cpal::Device,
    stream_config: &StreamConfig,
    sample_format: SampleFormat,
    audio_tx: Sender<Vec<f32>>,
    error_tx: Sender<String>,
) -> Result<Stream, AudioError> {
    match sample_format {
        SampleFormat::F32 => device
            .build_input_stream(
                stream_config,
                move |data: &[f32], _| {
                    let _ = audio_tx.send(data.to_vec());
                },
                move |err| {
                    let _ = error_tx.send(err.to_string());
                },
                None,
            )
            .map_err(AudioError::BuildStream),
        SampleFormat::I16 => device
            .build_input_stream(
                stream_config,
                move |data: &[i16], _| {
                    let converted = data
                        .iter()
                        .map(|sample| *sample as f32 / i16::MAX as f32)
                        .collect::<Vec<_>>();
                    let _ = audio_tx.send(converted);
                },
                move |err| {
                    let _ = error_tx.send(err.to_string());
                },
                None,
            )
            .map_err(AudioError::BuildStream),
        SampleFormat::U16 => device
            .build_input_stream(
                stream_config,
                move |data: &[u16], _| {
                    let converted = data
                        .iter()
                        .map(|sample| (*sample as f32 / u16::MAX as f32) * 2.0 - 1.0)
                        .collect::<Vec<_>>();
                    let _ = audio_tx.send(converted);
                },
                move |err| {
                    let _ = error_tx.send(err.to_string());
                },
                None,
            )
            .map_err(AudioError::BuildStream),
        _ => Err(AudioError::UnsupportedSampleFormat(format!(
            "{sample_format:?}"
        ))),
    }
}

fn build_input_level_stream(
    device: &cpal::Device,
    stream_config: &StreamConfig,
    sample_format: SampleFormat,
    level_tx: Sender<f32>,
    error_tx: Sender<String>,
) -> Result<Stream, AudioError> {
    match sample_format {
        SampleFormat::F32 => device
            .build_input_stream(
                stream_config,
                move |data: &[f32], _| {
                    let level = rms(data);
                    let _ = level_tx.send(level);
                },
                move |err| {
                    let _ = error_tx.send(err.to_string());
                },
                None,
            )
            .map_err(AudioError::BuildStream),
        SampleFormat::I16 => device
            .build_input_stream(
                stream_config,
                move |data: &[i16], _| {
                    let level = rms_i16(data);
                    let _ = level_tx.send(level);
                },
                move |err| {
                    let _ = error_tx.send(err.to_string());
                },
                None,
            )
            .map_err(AudioError::BuildStream),
        SampleFormat::U16 => device
            .build_input_stream(
                stream_config,
                move |data: &[u16], _| {
                    let level = rms_u16(data);
                    let _ = level_tx.send(level);
                },
                move |err| {
                    let _ = error_tx.send(err.to_string());
                },
                None,
            )
            .map_err(AudioError::BuildStream),
        _ => Err(AudioError::UnsupportedSampleFormat(format!(
            "{sample_format:?}"
        ))),
    }
}

pub fn downmix_to_mono(samples: &[f32], channels: u16) -> Vec<f32> {
    if channels <= 1 {
        return samples.to_vec();
    }

    let ch = channels as usize;
    samples
        .chunks(ch)
        .map(|chunk| chunk.iter().copied().sum::<f32>() / chunk.len() as f32)
        .collect()
}

pub fn resample_linear(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if samples.is_empty() || from_rate == to_rate {
        return samples.to_vec();
    }

    // Upsampling: linear interpolation is fine (no aliasing risk when widening).
    if to_rate > from_rate {
        let ratio = to_rate as f64 / from_rate as f64;
        let output_len = (samples.len() as f64 * ratio).round().max(1.0) as usize;
        let mut out = Vec::with_capacity(output_len);
        for idx in 0..output_len {
            let src_pos = idx as f64 / ratio;
            let left_idx = src_pos.floor() as usize;
            let right_idx = (left_idx + 1).min(samples.len() - 1);
            let alpha = (src_pos - left_idx as f64) as f32;
            let left = samples[left_idx];
            let right = samples[right_idx];
            out.push(left + alpha * (right - left));
        }
        return out;
    }

    // Downsampling: windowed integrator (box filter) — integrates source
    // samples over each output window, acting as a natural low-pass filter.
    // Stateless per-call, which is critical: `normalize_frame` calls this on
    // every small mic callback buffer; a stateful resampler (e.g. rubato's
    // SincFixedIn) would introduce a warmup transient and tail truncation at
    // every frame boundary, corrupting the audio.
    let src_per_out = from_rate as f64 / to_rate as f64;
    let output_len = (samples.len() as f64 / src_per_out).round().max(1.0) as usize;
    let mut out = Vec::with_capacity(output_len);

    for idx in 0..output_len {
        let window_start = idx as f64 * src_per_out;
        let window_end = (idx + 1) as f64 * src_per_out;
        let start_idx = (window_start as usize).min(samples.len());
        let end_idx = (window_end.ceil() as usize).min(samples.len());

        if start_idx >= end_idx {
            out.push(0.0);
            continue;
        }

        let mut sum = 0.0_f64;
        let mut weight = 0.0_f64;
        for i in start_idx..end_idx {
            let sample_start = i as f64;
            let sample_end = (i + 1) as f64;
            let overlap = sample_end.min(window_end) - sample_start.max(window_start);
            if overlap > 0.0 {
                sum += samples[i] as f64 * overlap;
                weight += overlap;
            }
        }

        if weight > 0.0 {
            out.push((sum / weight) as f32);
        } else {
            out.push(samples[start_idx]);
        }
    }

    out
}

#[derive(Debug, Clone, Copy)]
pub struct VadConfig {
    pub threshold: f32,
    pub min_speech_frames: usize,
    pub max_silence_frames: usize,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            threshold: 0.010,
            min_speech_frames: 3,
            max_silence_frames: 5,
        }
    }
}

#[derive(Debug, Clone)]
pub struct VadSegmenter {
    config: VadConfig,
    active_buffer: Vec<f32>,
    pre_roll_frames: Vec<Vec<f32>>,
    voiced_frames: usize,
    silence_frames: usize,
    in_speech: bool,
}

impl VadSegmenter {
    pub fn new(config: VadConfig) -> Self {
        Self {
            config,
            active_buffer: Vec::new(),
            pre_roll_frames: Vec::new(),
            voiced_frames: 0,
            silence_frames: 0,
            in_speech: false,
        }
    }

    pub fn push_frame(&mut self, frame: &[f32]) -> Option<Vec<f32>> {
        if frame.is_empty() {
            return None;
        }
        let is_voiced = rms(frame) >= self.config.threshold;

        if is_voiced {
            if !self.in_speech
                && self.voiced_frames == 0
                && self.active_buffer.is_empty()
                && !self.pre_roll_frames.is_empty()
            {
                for pre_roll in &self.pre_roll_frames {
                    self.active_buffer.extend_from_slice(pre_roll);
                }
            }
            self.pre_roll_frames.clear();
            self.voiced_frames += 1;
            self.silence_frames = 0;
            self.active_buffer.extend_from_slice(frame);
            if self.voiced_frames >= self.config.min_speech_frames {
                self.in_speech = true;
            }
            return None;
        }

        if self.in_speech {
            self.silence_frames += 1;
            self.active_buffer.extend_from_slice(frame);
            if self.silence_frames >= self.config.max_silence_frames {
                self.in_speech = false;
                self.voiced_frames = 0;
                self.silence_frames = 0;
                let mut segment = Vec::new();
                std::mem::swap(&mut segment, &mut self.active_buffer);
                self.pre_roll_frames.clear();
                return if segment.is_empty() {
                    None
                } else {
                    Some(segment)
                };
            }
        } else {
            self.active_buffer.clear();
            if self.pre_roll_frames.len() >= VAD_PRE_ROLL_FRAMES {
                self.pre_roll_frames.remove(0);
            }
            self.pre_roll_frames.push(frame.to_vec());
        }
        None
    }

    pub fn flush(&mut self) -> Option<Vec<f32>> {
        if self.active_buffer.is_empty() {
            return None;
        }
        self.in_speech = false;
        self.voiced_frames = 0;
        self.silence_frames = 0;
        self.pre_roll_frames.clear();
        let mut segment = Vec::new();
        std::mem::swap(&mut segment, &mut self.active_buffer);
        Some(segment)
    }
}

pub fn mock_audio_fixture_frames() -> Vec<Vec<f32>> {
    let mut frames = Vec::new();
    // Leading silence
    for _ in 0..5 {
        frames.push(vec![0.0; FRAME_SIZE]);
    }
    // Speech-like frames
    for i in 0..50 {
        let amplitude = 0.02 + (i % 7) as f32 * 0.003;
        let frame = (0..FRAME_SIZE)
            .map(|s| (((s as f32 * 0.04).sin()) * amplitude).clamp(-1.0, 1.0))
            .collect::<Vec<_>>();
        frames.push(frame);
    }
    // Trailing silence
    for _ in 0..8 {
        frames.push(vec![0.0; FRAME_SIZE]);
    }
    frames
}

fn rms(samples: &[f32]) -> f32 {
    let power_sum: f32 = samples.iter().map(|s| s * s).sum();
    (power_sum / samples.len() as f32).sqrt()
}

/// Compute the RMS threshold used by `trim_capture_edges` to decide which
/// frames are "voiced" enough to avoid trimming.
///
/// The threshold must adapt to the user's actual speaking volume. A fixed
/// threshold like `vad_threshold * 0.55 = 0.0055` is fine for normal-volume
/// speakers but catastrophically high for quiet speakers whose syllables
/// often sit at 0.003-0.008 RMS. With a fixed threshold the last voiced
/// frame gets pinned to the last LOUD syllable, so the 300 ms tail pad
/// starts there — any quieter trailing words are truncated, especially
/// after a mid-utterance silence gap when the user resumes quieter than
/// their opening.
///
/// Formula: take the lower of (15% of utterance RMS) and (55% of VAD
/// threshold), then clamp into a safe range. This mirrors the adaptive
/// logic in `condition_audio_v1::edge_threshold` and means loud speakers
/// get the expected ~0.0055 threshold while quiet speakers automatically
/// drop to ~0.0008 so their soft syllables are still seen as voiced.
pub fn adaptive_preserve_threshold(samples: &[f32], vad_threshold: f32) -> f32 {
    // Clamp bounds: 0.0008 is below typical ambient office noise (~0.001)
    // so we won't pick up mic hum as voiced. 0.02 is the ceiling used
    // previously; keep it to avoid cutting very loud content.
    const MIN_THRESHOLD: f32 = 0.0008;
    const MAX_THRESHOLD: f32 = 0.02;
    let global = rms(samples);
    let adaptive = global * 0.15;
    let vad_derived = vad_threshold * 0.55;
    adaptive.min(vad_derived).clamp(MIN_THRESHOLD, MAX_THRESHOLD)
}

fn trim_capture_edges(samples: &[f32], frame_size: usize, threshold: f32) -> Vec<f32> {
    if samples.is_empty() || frame_size == 0 {
        return Vec::new();
    }

    let mut first_voiced_frame = None::<usize>;
    let mut last_voiced_frame = None::<usize>;
    for (idx, frame) in samples.chunks(frame_size).enumerate() {
        if !frame.is_empty() && rms(frame) >= threshold {
            if first_voiced_frame.is_none() {
                first_voiced_frame = Some(idx);
            }
            last_voiced_frame = Some(idx);
        }
    }

    let Some(first) = first_voiced_frame else {
        return samples.to_vec();
    };
    let last = last_voiced_frame.unwrap_or(first);

    // Head padding: keep 4 leading silent frames (~80 ms) so leading
    // consonants (plosives like "p", "t", "k") are not clipped.
    const HEAD_PAD_FRAMES: usize = 4;
    // Tail padding: keep 15 trailing frames (~300 ms at 16 kHz with 20 ms
    // frames) past the last voiced frame so soft word endings ("s", "th",
    // "f", drifted "e") that drop below the VAD threshold still get
    // delivered to the decoder. This must stay >= TAIL_PAD_FRAMES in
    // trim_low_energy_edges (inference/audio_pipeline.rs) — otherwise the
    // inference-layer trim has nothing to pad with and soft word endings
    // are lost.
    const TAIL_PAD_FRAMES: usize = 15;
    let start_frame = first.saturating_sub(HEAD_PAD_FRAMES);
    let end_frame_exclusive = last.saturating_add(1).saturating_add(TAIL_PAD_FRAMES);
    let start = start_frame.saturating_mul(frame_size).min(samples.len());
    let end = end_frame_exclusive
        .saturating_mul(frame_size)
        .min(samples.len());
    if start >= end {
        return samples.to_vec();
    }
    samples[start..end].to_vec()
}

fn rms_i16(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let power_sum: f32 = samples
        .iter()
        .map(|s| {
            let normalized = *s as f32 / i16::MAX as f32;
            normalized * normalized
        })
        .sum();
    (power_sum / samples.len() as f32).sqrt()
}

fn rms_u16(samples: &[u16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let power_sum: f32 = samples
        .iter()
        .map(|s| {
            let normalized = (*s as f32 / u16::MAX as f32) * 2.0 - 1.0;
            normalized * normalized
        })
        .sum();
    (power_sum / samples.len() as f32).sqrt()
}

pub fn analyze_captured_segments(
    segments: &[Vec<f32>],
    sample_rate: u32,
    vad_threshold: f32,
) -> AudioQualityReport {
    let segment_count = segments.len() as u32;
    let total_samples_usize = segments.iter().map(|segment| segment.len()).sum::<usize>();
    let total_samples = total_samples_usize as u64;
    let duration_ms = if sample_rate > 0 && total_samples > 0 {
        ((total_samples as f64 / sample_rate as f64) * 1000.0).round() as u64
    } else {
        0
    };

    if total_samples_usize == 0 {
        return AudioQualityReport {
            sample_rate,
            segment_count,
            total_samples,
            duration_ms,
            rms: 0.0,
            peak: 0.0,
            clipping_ratio: 0.0,
            low_energy_frame_ratio: 1.0,
            estimated_snr_db: 0.0,
            quality: AudioQualityBand::Poor,
            issues: vec!["No audio chunks were captured.".to_string()],
            recommendations: vec![
                "Hold push-to-talk while speaking and verify microphone input in Windows settings."
                    .to_string(),
            ],
        };
    }

    let mut samples = Vec::with_capacity(total_samples_usize);
    for segment in segments {
        samples.extend_from_slice(segment);
    }

    let rms_value = rms(&samples);
    let peak = samples
        .iter()
        .fold(0.0_f32, |acc, sample| acc.max(sample.abs()));
    let clipping_count = samples.iter().filter(|sample| sample.abs() >= 0.98).count();
    let clipping_ratio = clipping_count as f32 / samples.len() as f32;

    let mut frame_rms = Vec::new();
    for frame in samples.chunks(FRAME_SIZE) {
        if !frame.is_empty() {
            frame_rms.push(rms(frame));
        }
    }

    let low_energy_threshold = (vad_threshold * 0.7).clamp(0.0025, 0.03);
    let low_energy_frames = frame_rms
        .iter()
        .filter(|value| **value < low_energy_threshold)
        .count();
    let low_energy_frame_ratio = if frame_rms.is_empty() {
        1.0
    } else {
        low_energy_frames as f32 / frame_rms.len() as f32
    };

    let mut sorted_frame_rms = frame_rms.clone();
    sorted_frame_rms
        .sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let noise_floor = percentile(&sorted_frame_rms, 0.15).max(1.0e-5);
    let speech_level = percentile(&sorted_frame_rms, 0.85).max(noise_floor);
    let estimated_snr_db = 20.0 * (speech_level / noise_floor).log10();

    let mut issues = Vec::new();
    let mut recommendations = Vec::new();
    let mut score = 100i32;

    if rms_value < 0.010 {
        score -= 40;
        issues.push("Captured audio chunks are very quiet.".to_string());
        recommendations
            .push("Increase microphone input gain or move closer to the microphone.".to_string());
    } else if rms_value < 0.018 {
        score -= 22;
        issues.push("Captured audio level is lower than recommended.".to_string());
        recommendations
            .push("Use a closer/wired mic path and keep speaking volume steady.".to_string());
    }

    if clipping_ratio > 0.030 {
        score -= 28;
        issues.push("Audio clipping is high and can distort words.".to_string());
        recommendations.push(
            "Reduce microphone gain or disable aggressive OS-level audio boosts.".to_string(),
        );
    } else if clipping_ratio > 0.010 {
        score -= 14;
        issues.push("Some clipping is present in the captured chunks.".to_string());
        recommendations.push("Lower input volume slightly to avoid peak distortion.".to_string());
    }

    if low_energy_frame_ratio > 0.70 {
        score -= 24;
        issues.push("Most frames are low-energy (speech is weak or intermittent).".to_string());
        recommendations.push(
            "Hold push-to-talk through the full utterance and avoid far-field microphone placement."
                .to_string(),
        );
    } else if low_energy_frame_ratio > 0.55 {
        score -= 12;
    }

    if estimated_snr_db < 8.0 {
        score -= 30;
        issues.push("Background noise is likely competing with speech.".to_string());
        recommendations.push(
            "Switch to a quieter input path (built-in or wired microphone over hands-free Bluetooth)."
                .to_string(),
        );
    } else if estimated_snr_db < 14.0 {
        score -= 12;
    }

    if peak < 0.03 {
        score -= 10;
        if !issues.iter().any(|issue| issue.contains("quiet")) {
            issues.push("Peak speech amplitude is very low.".to_string());
            recommendations.push(
                "Raise capture level so spoken words consistently clear background floor."
                    .to_string(),
            );
        }
    }

    let quality = if score >= 75 {
        AudioQualityBand::Good
    } else if score >= 50 {
        AudioQualityBand::Fair
    } else {
        AudioQualityBand::Poor
    };

    if issues.is_empty() {
        recommendations
            .push("Audio chunk quality looks healthy for on-device transcription.".to_string());
    }

    AudioQualityReport {
        sample_rate,
        segment_count,
        total_samples,
        duration_ms,
        rms: rms_value,
        peak,
        clipping_ratio,
        low_energy_frame_ratio,
        estimated_snr_db,
        quality,
        issues,
        recommendations,
    }
}

fn percentile(sorted: &[f32], quantile: f32) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    let clamped = quantile.clamp(0.0, 1.0);
    let index = ((sorted.len() - 1) as f32 * clamped).round() as usize;
    sorted[index]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adaptive_threshold_drops_for_quiet_speakers_so_soft_syllables_survive() {
        // Regression for the "quiet resumption after pause gets clipped" bug.
        //
        // A quiet speaker's utterance RMS sits around 0.005. With a fixed
        // threshold of vad_threshold * 0.55 = 0.0055 (the old behaviour),
        // soft syllables at 0.003-0.005 are flagged as silence and the
        // trim's last_voiced_frame stops at the last loud syllable. The
        // 300 ms tail pad then starts from that point and the quieter
        // trailing words get truncated.
        //
        // The adaptive formula must return a threshold LOWER than the
        // speaker's own soft syllables so they are preserved.
        let quiet_utterance = vec![0.005_f32; 320 * 50]; // ~1 s of quiet speech
        let vad_threshold = 0.01;
        let threshold = adaptive_preserve_threshold(&quiet_utterance, vad_threshold);
        assert!(
            threshold <= 0.0015,
            "quiet speaker (RMS 0.005) got threshold {threshold}; must be <= \
             0.0015 so their soft syllables are not trimmed",
        );
    }

    #[test]
    fn adaptive_threshold_stays_tight_for_normal_volume_speakers() {
        // A normal-volume speaker (RMS 0.04) should still get roughly the
        // old 0.0055 threshold so loud-speaker behaviour is unchanged.
        // The adaptive side gives 0.006, vad-derived gives 0.0055 — min
        // is 0.0055.
        let normal_utterance = vec![0.04_f32; 320 * 50];
        let threshold = adaptive_preserve_threshold(&normal_utterance, 0.01);
        assert!(
            (0.0045..=0.0060).contains(&threshold),
            "normal speaker (RMS 0.04) got unexpected threshold {threshold}; \
             expected near 0.0055 (no regression for loud speakers)",
        );
    }

    #[test]
    fn adaptive_threshold_never_drops_below_ambient_noise_floor() {
        // Silent utterance (just ambient mic noise) must NOT send the
        // threshold to effectively zero, or trim_capture_edges would
        // flag every frame as voiced and never trim anything.
        let silence = vec![0.0005_f32; 320 * 50];
        let threshold = adaptive_preserve_threshold(&silence, 0.01);
        assert!(
            threshold >= 0.0008,
            "silence threshold {threshold} fell below the 0.0008 ambient \
             noise floor — would defeat trimming entirely",
        );
    }

    #[test]
    fn release_tail_min_wait_is_long_enough_for_soft_consonants() {
        // Root-cause regression for the "last 2-3 words missing" bug that
        // persisted after trim_capture_edges was bumped to a 300 ms tail
        // pad.
        //
        // Capture flow: when the user releases the hotkey, the loop waits
        // for `RELEASE_TAIL_MIN_WAIT_MS`, then breaks if no voiced frame
        // has been heard in the last `RELEASE_TAIL_SILENCE_CONFIRM_MS`.
        //
        // Soft word endings ("s", "th", "f", drifted "d") trail below the
        // RMS VAD threshold, so `post_release_last_voiced_at` is never set
        // during the soft tail. Result: the loop always breaks at exactly
        // `RELEASE_TAIL_MIN_WAIT_MS` for soft endings — that constant IS
        // the effective post-release capture window for the problematic
        // case.
        //
        // It must therefore be >= the 300 ms tail pad in trim_capture_edges,
        // or the pad has no samples to operate on and the downstream
        // trim_low_energy_edges pad is equally useless.
        assert!(
            RELEASE_TAIL_MIN_WAIT_MS >= 300,
            "RELEASE_TAIL_MIN_WAIT_MS={} is less than the 300 ms tail pad \
             used by trim_capture_edges. Soft word endings will be clipped \
             before the pad can restore them.",
            RELEASE_TAIL_MIN_WAIT_MS
        );
    }

    #[test]
    fn stereo_downmix_is_averaged() {
        let stereo = vec![1.0, -1.0, 0.5, 0.5];
        let mono = downmix_to_mono(&stereo, 2);
        assert_eq!(mono, vec![0.0, 0.5]);
    }

    #[test]
    fn trim_capture_edges_preserves_generous_tail_padding_for_soft_word_endings() {
        // Simulate two silent head frames, three voiced frames, then a long
        // trailing silence. The trim logic should keep enough trailing frames
        // past the last voiced frame to capture soft word endings ("s", "th")
        // that drop below the VAD threshold. Expected: >= 8 trailing frames
        // are preserved after the last voiced frame.
        let frame_size = FRAME_SIZE;
        let mut samples = vec![0.0_f32; frame_size * 2];
        samples.extend(vec![0.05_f32; frame_size * 3]);
        samples.extend(vec![0.0_f32; frame_size * 15]);

        let trimmed = trim_capture_edges(&samples, frame_size, 0.01);

        let last_voiced_frame = 4; // frames 2..=4 are voiced
        let min_expected_trailing_frames = 8;
        let min_expected_len = (last_voiced_frame + min_expected_trailing_frames) * frame_size;
        assert!(
            trimmed.len() >= min_expected_len,
            "expected at least {min_expected_len} samples of trailing padding, got {}",
            trimmed.len()
        );
    }

    #[test]
    fn trim_capture_edges_tail_padding_matches_inference_layer_generosity() {
        // Regression: the capture-layer tail trim must keep at least 15
        // frames (300 ms at 16 kHz / 20 ms frames) past the last voiced
        // frame, matching trim_low_energy_edges in inference/audio_pipeline.rs.
        // Otherwise soft word endings are clipped at the capture layer
        // before the inference pipeline ever sees them, and the 300 ms
        // padding in the inference-layer trim cannot recover samples that
        // were already truncated upstream.
        let frame_size = FRAME_SIZE;
        let mut samples = vec![0.0_f32; frame_size * 2];
        samples.extend(vec![0.05_f32; frame_size * 3]);
        samples.extend(vec![0.0_f32; frame_size * 30]);

        let trimmed = trim_capture_edges(&samples, frame_size, 0.01);

        let last_voiced_frame = 4; // frames 2..=4 are voiced (0-indexed)
        let min_tail_frames = 15;
        let min_expected_len = (last_voiced_frame + 1 + min_tail_frames) * frame_size;
        let actual_tail_frames = (trimmed.len() / frame_size)
            .saturating_sub(last_voiced_frame + 1);
        assert!(
            trimmed.len() >= min_expected_len,
            "capture-layer tail pad too aggressive: kept only {actual_tail_frames} frames \
             past last voiced frame (len={}); need at least {min_tail_frames} frames \
             (len >= {min_expected_len}) to match inference-layer trim",
            trimmed.len()
        );
    }

    #[test]
    fn trim_capture_edges_preserves_extra_head_padding_for_leading_consonants() {
        // Capture should keep at least two silent frames before the first
        // voiced frame so leading consonants are not clipped.
        let frame_size = FRAME_SIZE;
        let mut samples = vec![0.0_f32; frame_size * 4];
        samples.extend(vec![0.05_f32; frame_size * 3]);
        samples.extend(vec![0.0_f32; frame_size * 4]);

        let trimmed = trim_capture_edges(&samples, frame_size, 0.01);

        // Trim should never chop more than 2 head frames (leaving >= 9 frames).
        let min_expected_len = (samples.len() / frame_size - 2) * frame_size;
        assert!(
            trimmed.len() >= min_expected_len,
            "head padding eaten too much: got {} samples, expected at least {}",
            trimmed.len(),
            min_expected_len
        );
    }

    #[test]
    fn resample_changes_length() {
        let samples = vec![0.0, 1.0, 0.0, -1.0];
        let out = resample_linear(&samples, 8_000, 16_000);
        assert!(out.len() > samples.len());
    }

    #[test]
    fn resample_attenuates_frequencies_above_target_nyquist() {
        // Generate a 12 kHz sine at 48 kHz. After downsampling to 16 kHz
        // (whose Nyquist is 8 kHz) the 12 kHz content must be strongly
        // attenuated. Pure linear interpolation aliases it back into audible
        // band; a proper low-pass resampler kills it.
        let duration_samples = 48_000 / 10; // 100 ms of audio
        let high_freq = (0..duration_samples)
            .map(|i| (2.0 * std::f32::consts::PI * 12_000.0 * i as f32 / 48_000.0).sin() * 0.5)
            .collect::<Vec<f32>>();

        let resampled = resample_linear(&high_freq, 48_000, 16_000);

        let input_rms = (high_freq.iter().map(|s| s * s).sum::<f32>() / high_freq.len() as f32)
            .sqrt();
        let output_rms = (resampled.iter().map(|s| s * s).sum::<f32>() / resampled.len() as f32)
            .sqrt();

        assert!(
            output_rms < input_rms * 0.35,
            "above-Nyquist content should be strongly attenuated: input_rms={input_rms:.4}, \
             output_rms={output_rms:.4}"
        );
    }

    #[test]
    fn resample_preserves_speech_band_content() {
        // A 1 kHz tone is well inside the speech band and must pass through
        // the resampler with similar energy (no excessive attenuation).
        let duration_samples = 48_000 / 10;
        let speech_tone = (0..duration_samples)
            .map(|i| (2.0 * std::f32::consts::PI * 1_000.0 * i as f32 / 48_000.0).sin() * 0.5)
            .collect::<Vec<f32>>();

        let resampled = resample_linear(&speech_tone, 48_000, 16_000);

        let input_rms = (speech_tone.iter().map(|s| s * s).sum::<f32>()
            / speech_tone.len() as f32)
            .sqrt();
        let output_rms = (resampled.iter().map(|s| s * s).sum::<f32>() / resampled.len() as f32)
            .sqrt();

        assert!(
            output_rms > input_rms * 0.75,
            "speech-band content should pass through with minimal loss: \
             input_rms={input_rms:.4}, output_rms={output_rms:.4}"
        );
    }

    #[test]
    fn vad_emits_segment_after_silence_tail() {
        let mut vad = VadSegmenter::new(VadConfig::default());
        let mut segments = Vec::new();
        for frame in mock_audio_fixture_frames() {
            if let Some(segment) = vad.push_frame(&frame) {
                segments.push(segment);
            }
        }
        if let Some(segment) = vad.flush() {
            segments.push(segment);
        }
        assert!(!segments.is_empty());
        assert!(segments[0].iter().any(|s| s.abs() > 0.01));
    }

    #[test]
    fn vad_pre_roll_preserves_quiet_onset() {
        let mut vad = VadSegmenter::new(VadConfig {
            threshold: 0.010,
            min_speech_frames: 2,
            max_silence_frames: 2,
        });
        let quiet_onset = vec![0.006_f32; FRAME_SIZE];
        let voiced = vec![0.022_f32; FRAME_SIZE];
        let silence = vec![0.0_f32; FRAME_SIZE];

        assert!(vad.push_frame(&quiet_onset).is_none());
        assert!(vad.push_frame(&voiced).is_none());
        assert!(vad.push_frame(&voiced).is_none());
        assert!(vad.push_frame(&silence).is_none());
        let segment = vad
            .push_frame(&silence)
            .or_else(|| vad.flush())
            .expect("segment should include onset");

        assert!(segment.len() >= FRAME_SIZE * 3);
        assert!(segment[..FRAME_SIZE].iter().all(|sample| *sample >= 0.005));
    }

    #[test]
    fn capture_options_default_is_phase_one_sane() {
        let options = CaptureOptions::default();
        assert!(options.max_capture_duration >= Duration::from_secs(10));
        assert!(options.silence_timeout >= Duration::from_millis(500));
        assert!(options.release_tail <= Duration::from_millis(100));
    }

    #[test]
    fn capture_waits_for_explicit_stop_after_speech_when_release_tail_enabled() {
        let service = AudioCaptureService::default();
        let (audio_tx, audio_rx) = std::sync::mpsc::channel::<Vec<f32>>();
        let (_error_tx, error_rx) = std::sync::mpsc::channel::<String>();
        let options = CaptureOptions {
            vad_config: VadConfig {
                threshold: 0.01,
                ..VadConfig::default()
            },
            max_capture_duration: Duration::from_millis(140),
            silence_timeout: Duration::from_millis(25),
            release_tail: Duration::from_millis(120),
            preserve_full_capture: false,
        };

        audio_tx
            .send(vec![0.08_f32; FRAME_SIZE * 3])
            .expect("test frame should be queued");

        let started = Instant::now();
        let segments = service
            .collect_segments_from_stream(
                service.target_sample_rate,
                1,
                audio_rx,
                error_rx,
                options,
                || false,
                || false,
                |_normalized_chunk, _voiced_chunk| {},
            )
            .expect("capture should complete");

        assert!(
            started.elapsed() >= Duration::from_millis(90),
            "capture ended before explicit stop/max duration guard"
        );
        assert!(!segments.is_empty());
    }

    #[test]
    fn release_tail_exits_fast_when_user_released_after_silence() {
        let service = AudioCaptureService::default();
        let (audio_tx, audio_rx) = std::sync::mpsc::channel::<Vec<f32>>();
        let (_error_tx, error_rx) = std::sync::mpsc::channel::<String>();
        let options = CaptureOptions {
            vad_config: VadConfig {
                threshold: 0.01,
                ..VadConfig::default()
            },
            max_capture_duration: Duration::from_millis(2_000),
            silence_timeout: Duration::from_millis(1_500),
            release_tail: Duration::from_millis(600),
            preserve_full_capture: false,
        };

        // Speech at the very start, then silence; the user "releases" 520 ms
        // in — i.e. well past PRE_RELEASE_SILENCE_PAD_MS after the last voice.
        audio_tx
            .send(vec![0.08_f32; FRAME_SIZE * 3])
            .expect("test frame should be queued");

        let started = Instant::now();
        let stop_clock = started;
        let segments = service
            .collect_segments_from_stream(
                service.target_sample_rate,
                1,
                audio_rx,
                error_rx,
                options,
                || false,
                move || stop_clock.elapsed() >= Duration::from_millis(520),
                |_normalized_chunk, _voiced_chunk| {},
            )
            .expect("capture should complete");

        let elapsed = started.elapsed();
        assert!(!segments.is_empty());
        assert!(
            elapsed >= Duration::from_millis(600),
            "fast exit may not fire before the minimum fast wait (got {elapsed:?})"
        );
        // Slow path would exit no earlier than 520 + 300 = 820 ms; the fast
        // path exits around 520 + 120 = 640 ms.
        assert!(
            elapsed <= Duration::from_millis(780),
            "released-after-silence capture should fast-exit the tail (got {elapsed:?})"
        );
    }

    #[test]
    fn release_tail_keeps_full_wait_when_voice_runs_into_release() {
        let service = AudioCaptureService::default();
        let (audio_tx, audio_rx) = std::sync::mpsc::channel::<Vec<f32>>();
        let (_error_tx, error_rx) = std::sync::mpsc::channel::<String>();
        let options = CaptureOptions {
            vad_config: VadConfig {
                threshold: 0.01,
                ..VadConfig::default()
            },
            max_capture_duration: Duration::from_millis(2_000),
            silence_timeout: Duration::from_millis(1_500),
            release_tail: Duration::from_millis(600),
            preserve_full_capture: false,
        };

        // Voiced chunks keep arriving until ~480 ms; the stop lands at 520 ms,
        // only ~40 ms after the last voice — soft endings may still be in
        // flight, so the tail must keep its full minimum wait.
        let feeder = std::thread::spawn(move || {
            for _ in 0..12 {
                if audio_tx.send(vec![0.08_f32; FRAME_SIZE]).is_err() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(40));
            }
            // Keep the channel open (silent) so capture ends via the tail
            // logic rather than a sender disconnect.
            std::thread::sleep(Duration::from_millis(700));
        });

        let started = Instant::now();
        let stop_clock = started;
        let segments = service
            .collect_segments_from_stream(
                service.target_sample_rate,
                1,
                audio_rx,
                error_rx,
                options,
                || false,
                move || stop_clock.elapsed() >= Duration::from_millis(520),
                |_normalized_chunk, _voiced_chunk| {},
            )
            .expect("capture should complete");
        feeder.join().expect("feeder thread should finish");

        let elapsed = started.elapsed();
        assert!(!segments.is_empty());
        assert!(
            elapsed >= Duration::from_millis(800),
            "voice near release must keep the full tail minimum wait (got {elapsed:?})"
        );
    }

    #[test]
    fn capture_preserve_full_capture_requires_speech_evidence() {
        let service = AudioCaptureService::default();
        let (audio_tx, audio_rx) = std::sync::mpsc::channel::<Vec<f32>>();
        let (_error_tx, error_rx) = std::sync::mpsc::channel::<String>();
        let options = CaptureOptions {
            vad_config: VadConfig {
                threshold: 0.08,
                ..VadConfig::default()
            },
            max_capture_duration: Duration::from_millis(200),
            silence_timeout: Duration::from_millis(40),
            release_tail: Duration::from_millis(0),
            preserve_full_capture: true,
        };

        audio_tx
            .send(vec![0.03_f32; FRAME_SIZE * 3])
            .expect("test frame should be queued");
        drop(audio_tx);

        let segments = service
            .collect_segments_from_stream(
                service.target_sample_rate,
                1,
                audio_rx,
                error_rx,
                options,
                || false,
                || false,
                |_normalized_chunk, _voiced_chunk| {},
            )
            .expect("capture should complete");

        assert!(segments.is_empty());
    }

    #[test]
    fn capture_preserve_full_capture_keeps_audio_when_speech_is_detected() {
        let service = AudioCaptureService::default();
        let (audio_tx, audio_rx) = std::sync::mpsc::channel::<Vec<f32>>();
        let (_error_tx, error_rx) = std::sync::mpsc::channel::<String>();
        let options = CaptureOptions {
            vad_config: VadConfig {
                threshold: 0.04,
                ..VadConfig::default()
            },
            max_capture_duration: Duration::from_millis(200),
            silence_timeout: Duration::from_millis(40),
            release_tail: Duration::from_millis(0),
            preserve_full_capture: true,
        };

        audio_tx
            .send(vec![0.06_f32; FRAME_SIZE * 4])
            .expect("test frame should be queued");
        drop(audio_tx);

        let segments = service
            .collect_segments_from_stream(
                service.target_sample_rate,
                1,
                audio_rx,
                error_rx,
                options,
                || false,
                || false,
                |_normalized_chunk, _voiced_chunk| {},
            )
            .expect("capture should complete");

        assert_eq!(segments.len(), 1);
        assert!(!segments[0].is_empty());
    }

    #[test]
    fn quality_report_flags_quiet_audio() {
        let quiet_segment = (0..(16_000 / 2))
            .map(|i| ((i as f32 * 0.07).sin()) * 0.004)
            .collect::<Vec<_>>();
        let report = analyze_captured_segments(&[quiet_segment], TARGET_SAMPLE_RATE, 0.014);

        assert_eq!(report.quality, AudioQualityBand::Poor);
        assert!(report.issues.iter().any(|issue| issue.contains("quiet")));
    }

    #[test]
    fn quality_report_flags_clipping_audio() {
        let clipped = vec![1.0_f32; 16_000];
        let report = analyze_captured_segments(&[clipped], TARGET_SAMPLE_RATE, 0.014);

        assert!(report.clipping_ratio > 0.5);
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.to_lowercase().contains("clipping")));
    }

    #[test]
    fn quality_report_detects_healthy_signal() {
        let clean = (0..16_000)
            .map(|i| ((i as f32 * 0.08).sin()) * 0.08)
            .collect::<Vec<_>>();
        let report = analyze_captured_segments(&[clean], TARGET_SAMPLE_RATE, 0.014);

        assert!(matches!(
            report.quality,
            AudioQualityBand::Good | AudioQualityBand::Fair
        ));
        assert!(report.rms > 0.02);
    }
}
