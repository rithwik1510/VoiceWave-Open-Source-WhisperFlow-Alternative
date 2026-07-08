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

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Sender},
        Arc, Mutex, OnceLock,
    },
    time::{Duration, Instant},
};

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

fn mix_next_frame(voices: &mut Vec<ActiveVoice>) -> f32 {
    let mut sum = 0.0_f32;
    for voice in voices.iter_mut() {
        if let Some(sample) = voice.samples.get(voice.position) {
            sum += sample * VOICE_GAIN;
            voice.position += 1;
        }
    }
    voices.retain(|voice| voice.position < voice.samples.len());
    sum.clamp(-1.0, 1.0)
}

const WORKER_READY_TIMEOUT: Duration = Duration::from_secs(2);
const CUE_ACCEPT_TIMEOUT: Duration = Duration::from_millis(40);
const PREWARM_RETRY_BACKOFF: Duration = Duration::from_millis(300);

trait CueWorker: Send {
    fn device_fingerprint(&self) -> &str;
    fn is_healthy(&self) -> bool;
    fn play(&self, cue: CueSound) -> Result<(), String>;
    fn shutdown(&mut self);
}

trait CueWorkerFactory: Send + Sync {
    fn current_device_fingerprint(&self) -> Result<String, String>;
    fn spawn(&self) -> Result<Box<dyn CueWorker>, String>;
}

struct CueSupervisor {
    factory: Box<dyn CueWorkerFactory>,
    worker: Option<Box<dyn CueWorker>>,
    last_start_failure: Option<Instant>,
}

impl CueSupervisor {
    fn new(factory: Box<dyn CueWorkerFactory>) -> Self {
        Self {
            factory,
            worker: None,
            last_start_failure: None,
        }
    }

    fn prewarm(&mut self) -> Result<(), String> {
        self.ensure_worker(false)
    }

    fn play(&mut self, cue: CueSound) -> Result<(), String> {
        self.ensure_worker(true)?;
        if self
            .worker
            .as_ref()
            .ok_or_else(|| "cue worker unavailable after startup".to_string())?
            .play(cue)
            .is_ok()
        {
            return Ok(());
        }

        // The worker can fail between the health probe and send. Retire it,
        // synchronously establish a ready replacement on the current default
        // device, and retry this same cue exactly once.
        self.retire_worker();
        self.ensure_worker(true)?;
        self.worker
            .as_ref()
            .ok_or_else(|| "cue worker unavailable after recovery".to_string())?
            .play(cue)
    }

    fn ensure_worker(&mut self, user_initiated: bool) -> Result<(), String> {
        let current_device = match self.factory.current_device_fingerprint() {
            Ok(fingerprint) => fingerprint,
            Err(err) => {
                // Windows can briefly report no default endpoint while an
                // output device is being switched. A stream that is still
                // healthy can keep accepting cues during that transition;
                // discarding it here would turn a transient probe failure
                // into an avoidable silent hotkey.
                if self
                    .worker
                    .as_ref()
                    .is_some_and(|worker| worker.is_healthy())
                {
                    return Ok(());
                }
                return Err(err);
            }
        };
        let reusable = self.worker.as_ref().is_some_and(|worker| {
            worker.is_healthy() && worker.device_fingerprint() == current_device
        });
        if reusable {
            return Ok(());
        }
        self.retire_worker();

        if !user_initiated
            && self
                .last_start_failure
                .is_some_and(|failed_at| failed_at.elapsed() < PREWARM_RETRY_BACKOFF)
        {
            return Err("cue worker restart is in backoff".to_string());
        }

        match self.factory.spawn() {
            Ok(worker) => {
                self.worker = Some(worker);
                self.last_start_failure = None;
                Ok(())
            }
            Err(err) => {
                self.last_start_failure = Some(Instant::now());
                Err(err)
            }
        }
    }

    fn retire_worker(&mut self) {
        if let Some(mut worker) = self.worker.take() {
            worker.shutdown();
        }
    }
}

impl Drop for CueSupervisor {
    fn drop(&mut self) {
        self.retire_worker();
    }
}

static CUE_SERVICE: OnceLock<Mutex<CueSupervisor>> = OnceLock::new();

fn service() -> &'static Mutex<CueSupervisor> {
    CUE_SERVICE.get_or_init(|| Mutex::new(CueSupervisor::new(Box::new(CpalWorkerFactory))))
}

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
    if !cues_enabled() {
        return;
    }
    if let Ok(mut supervisor) = service().lock() {
        if let Err(err) = supervisor.prewarm() {
            eprintln!("voicewave: cue prewarm deferred: {err}");
        }
    }
}

pub fn play(cue: CueSound) {
    if !cues_enabled() {
        return;
    }
    match service().lock() {
        Ok(mut supervisor) => {
            if let Err(err) = supervisor.play(cue) {
                eprintln!("voicewave: cue playback unavailable after recovery: {err}");
            }
        }
        Err(_) => eprintln!("voicewave: cue supervisor lock is poisoned"),
    }
}

struct CpalWorkerFactory;

impl CueWorkerFactory for CpalWorkerFactory {
    fn current_device_fingerprint(&self) -> Result<String, String> {
        default_output_device_and_fingerprint().map(|(_, fingerprint)| fingerprint)
    }

    fn spawn(&self) -> Result<Box<dyn CueWorker>, String> {
        CpalCueWorker::spawn().map(|worker| Box::new(worker) as Box<dyn CueWorker>)
    }
}

enum WorkerCommand {
    Play {
        cue: CueSound,
        accepted: mpsc::SyncSender<Result<(), String>>,
    },
    Shutdown,
}

struct CpalCueWorker {
    sender: Sender<WorkerCommand>,
    healthy: Arc<AtomicBool>,
    device_fingerprint: String,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl CpalCueWorker {
    fn spawn() -> Result<Self, String> {
        let (command_tx, command_rx) = mpsc::channel::<WorkerCommand>();
        let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<String, String>>(1);
        let healthy = Arc::new(AtomicBool::new(false));
        let healthy_for_thread = Arc::clone(&healthy);
        let thread = std::thread::Builder::new()
            .name("voicewave-cue-audio".to_string())
            .spawn(move || audio_thread(command_rx, ready_tx, healthy_for_thread))
            .map_err(|err| format!("failed to spawn cue audio thread: {err}"))?;

        match ready_rx.recv_timeout(WORKER_READY_TIMEOUT) {
            Ok(Ok(device_fingerprint)) => Ok(Self {
                sender: command_tx,
                healthy,
                device_fingerprint,
                thread: Some(thread),
            }),
            Ok(Err(err)) => {
                let _ = thread.join();
                Err(err)
            }
            Err(err) => {
                // Do not leave a late-starting worker resident. The thread
                // may still be inside a slow driver call, so signal shutdown
                // without joining (which would defeat the bounded timeout).
                // Once initialization returns it will consume the command
                // and exit; dropping the handle merely detaches that cleanup.
                let _ = command_tx.send(WorkerCommand::Shutdown);
                drop(thread);
                Err(format!("cue worker readiness timed out: {err}"))
            }
        }
    }
}

impl CueWorker for CpalCueWorker {
    fn device_fingerprint(&self) -> &str {
        &self.device_fingerprint
    }

    fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::Acquire)
    }

    fn play(&self, cue: CueSound) -> Result<(), String> {
        if !self.is_healthy() {
            return Err("cue output stream is unhealthy".to_string());
        }
        let (accepted_tx, accepted_rx) = mpsc::sync_channel(1);
        self.sender
            .send(WorkerCommand::Play {
                cue,
                accepted: accepted_tx,
            })
            .map_err(|_| "cue audio worker has exited".to_string())?;
        accepted_rx
            .recv_timeout(CUE_ACCEPT_TIMEOUT)
            .map_err(|err| format!("cue audio worker did not accept playback: {err}"))?
    }

    fn shutdown(&mut self) {
        self.healthy.store(false, Ordering::Release);
        let _ = self.sender.send(WorkerCommand::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn default_output_device_and_fingerprint() -> Result<(cpal::Device, String), String> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| "no default output device".to_string())?;
    let supported = device
        .default_output_config()
        .map_err(|err| format!("no default output config: {err}"))?;
    let device_name = device
        .name()
        .unwrap_or_else(|_| "unknown-output".to_string());
    let fingerprint = format!(
        "{}|{}|{}|{:?}",
        device_name,
        supported.sample_rate().0,
        supported.channels(),
        supported.sample_format()
    );
    Ok((device, fingerprint))
}

fn audio_thread(
    rx: mpsc::Receiver<WorkerCommand>,
    ready: mpsc::SyncSender<Result<String, String>>,
    healthy: Arc<AtomicBool>,
) {
    let (device, device_fingerprint) = match default_output_device_and_fingerprint() {
        Ok(value) => value,
        Err(err) => {
            let _ = ready.send(Err(err));
            return;
        }
    };
    let supported = match device.default_output_config() {
        Ok(config) => config,
        Err(err) => {
            let _ = ready.send(Err(format!("no default output config: {err}")));
            return;
        }
    };
    let sample_rate = supported.sample_rate().0;
    let channels = supported.channels() as usize;
    let sample_format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();

    let open_samples = Arc::new(decode_wav_to_rate(CUE_OPEN_WAV, sample_rate));
    let close_samples = Arc::new(decode_wav_to_rate(CUE_CLOSE_WAV, sample_rate));

    let voices: Arc<Mutex<Vec<ActiveVoice>>> = Arc::new(Mutex::new(Vec::new()));

    let error_handler = || {
        let healthy = Arc::clone(&healthy);
        move |err| {
            healthy.store(false, Ordering::Release);
            eprintln!("voicewave: cue output stream error: {err}");
        }
    };
    let stream = match sample_format {
        cpal::SampleFormat::F32 => {
            let voices_for_callback = Arc::clone(&voices);
            device.build_output_stream(
                &config,
                move |data: &mut [f32], _| {
                    let mut voices = match voices_for_callback.lock() {
                        Ok(guard) => guard,
                        Err(_) => return,
                    };
                    for frame in data.chunks_mut(channels) {
                        let value = mix_next_frame(&mut voices);
                        for channel_sample in frame {
                            *channel_sample = value;
                        }
                    }
                },
                error_handler(),
                None,
            )
        }
        cpal::SampleFormat::I16 => {
            let voices_for_callback = Arc::clone(&voices);
            device.build_output_stream(
                &config,
                move |data: &mut [i16], _| {
                    let mut voices = match voices_for_callback.lock() {
                        Ok(guard) => guard,
                        Err(_) => return,
                    };
                    for frame in data.chunks_mut(channels) {
                        let value = (mix_next_frame(&mut voices) * i16::MAX as f32) as i16;
                        for channel_sample in frame {
                            *channel_sample = value;
                        }
                    }
                },
                error_handler(),
                None,
            )
        }
        cpal::SampleFormat::U16 => {
            let voices_for_callback = Arc::clone(&voices);
            device.build_output_stream(
                &config,
                move |data: &mut [u16], _| {
                    let mut voices = match voices_for_callback.lock() {
                        Ok(guard) => guard,
                        Err(_) => return,
                    };
                    for frame in data.chunks_mut(channels) {
                        let value = mix_next_frame(&mut voices);
                        let encoded = ((value * 0.5 + 0.5) * u16::MAX as f32) as u16;
                        for channel_sample in frame {
                            *channel_sample = encoded;
                        }
                    }
                },
                error_handler(),
                None,
            )
        }
        other => {
            let _ = ready.send(Err(format!("unsupported output sample format {other:?}")));
            return;
        }
    };

    let stream = match stream {
        Ok(stream) => stream,
        Err(err) => {
            let _ = ready.send(Err(format!("stream build failed: {err}")));
            return;
        }
    };
    if let Err(err) = stream.play() {
        let _ = ready.send(Err(format!("stream start failed: {err}")));
        return;
    }
    healthy.store(true, Ordering::Release);
    let _ = ready.send(Ok(device_fingerprint));

    while let Ok(command) = rx.recv() {
        match command {
            WorkerCommand::Play { cue, accepted } => {
                if !healthy.load(Ordering::Acquire) {
                    let _ = accepted.send(Err("cue output stream became unhealthy".to_string()));
                    break;
                }
                let samples = match cue {
                    CueSound::Open => open_samples.clone(),
                    CueSound::Close => close_samples.clone(),
                };
                match voices.lock() {
                    Ok(mut voices) => {
                        voices.push(ActiveVoice {
                            samples,
                            position: 0,
                        });
                        let _ = accepted.send(Ok(()));
                    }
                    Err(_) => {
                        healthy.store(false, Ordering::Release);
                        let _ = accepted.send(Err("cue voice mixer lock is poisoned".to_string()));
                        break;
                    }
                }
            }
            WorkerCommand::Shutdown => break,
        }
    }
    healthy.store(false, Ordering::Release);
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
    use super::*;

    #[derive(Debug)]
    struct FakeState {
        fingerprint: String,
        healthy: bool,
        device_probe_failures: usize,
        spawn_failures: usize,
        play_failures: usize,
        spawn_count: usize,
        shutdown_count: usize,
        played: Vec<CueSound>,
    }

    #[derive(Clone)]
    struct FakeFactory {
        state: Arc<Mutex<FakeState>>,
    }

    struct FakeWorker {
        state: Arc<Mutex<FakeState>>,
        fingerprint: String,
    }

    impl CueWorkerFactory for FakeFactory {
        fn current_device_fingerprint(&self) -> Result<String, String> {
            let mut state = self.state.lock().expect("fake state");
            if state.device_probe_failures > 0 {
                state.device_probe_failures -= 1;
                return Err("injected device probe failure".to_string());
            }
            Ok(state.fingerprint.clone())
        }

        fn spawn(&self) -> Result<Box<dyn CueWorker>, String> {
            let mut state = self.state.lock().expect("fake state");
            state.spawn_count += 1;
            if state.spawn_failures > 0 {
                state.spawn_failures -= 1;
                return Err("injected spawn failure".to_string());
            }
            state.healthy = true;
            Ok(Box::new(FakeWorker {
                state: Arc::clone(&self.state),
                fingerprint: state.fingerprint.clone(),
            }))
        }
    }

    impl CueWorker for FakeWorker {
        fn device_fingerprint(&self) -> &str {
            &self.fingerprint
        }

        fn is_healthy(&self) -> bool {
            self.state.lock().expect("fake state").healthy
        }

        fn play(&self, cue: CueSound) -> Result<(), String> {
            let mut state = self.state.lock().expect("fake state");
            if state.play_failures > 0 {
                state.play_failures -= 1;
                state.healthy = false;
                return Err("injected play failure".to_string());
            }
            state.played.push(cue);
            Ok(())
        }

        fn shutdown(&mut self) {
            let mut state = self.state.lock().expect("fake state");
            state.shutdown_count += 1;
            state.healthy = false;
        }
    }

    fn fake_supervisor() -> (CueSupervisor, Arc<Mutex<FakeState>>) {
        let state = Arc::new(Mutex::new(FakeState {
            fingerprint: "speakers-a".to_string(),
            healthy: false,
            device_probe_failures: 0,
            spawn_failures: 0,
            play_failures: 0,
            spawn_count: 0,
            shutdown_count: 0,
            played: Vec::new(),
        }));
        (
            CueSupervisor::new(Box::new(FakeFactory {
                state: Arc::clone(&state),
            })),
            state,
        )
    }

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

    #[test]
    fn prewarm_keeps_one_ready_worker() {
        let (mut supervisor, state) = fake_supervisor();
        supervisor.prewarm().expect("prewarm");
        supervisor.prewarm().expect("reuse prewarm");
        assert_eq!(state.lock().expect("state").spawn_count, 1);
    }

    #[test]
    fn default_device_change_restarts_before_playing_the_cue() {
        let (mut supervisor, state) = fake_supervisor();
        supervisor.prewarm().expect("prewarm");
        state.lock().expect("state").fingerprint = "headphones-b".to_string();
        supervisor.play(CueSound::Open).expect("play after switch");
        let state = state.lock().expect("state");
        assert_eq!(state.spawn_count, 2);
        assert_eq!(state.shutdown_count, 1);
        assert_eq!(state.played, vec![CueSound::Open]);
    }

    #[test]
    fn dead_worker_is_replaced_before_the_next_cue() {
        let (mut supervisor, state) = fake_supervisor();
        supervisor.prewarm().expect("prewarm");
        state.lock().expect("state").healthy = false;
        supervisor.play(CueSound::Close).expect("recovered play");
        let state = state.lock().expect("state");
        assert_eq!(state.spawn_count, 2);
        assert_eq!(state.played, vec![CueSound::Close]);
    }

    #[test]
    fn transient_device_probe_failure_keeps_a_healthy_stream_audible() {
        let (mut supervisor, state) = fake_supervisor();
        supervisor.prewarm().expect("prewarm");
        state.lock().expect("state").device_probe_failures = 1;
        supervisor
            .play(CueSound::Open)
            .expect("healthy stream should survive endpoint transition");
        let state = state.lock().expect("state");
        assert_eq!(state.spawn_count, 1);
        assert_eq!(state.played, vec![CueSound::Open]);
    }

    #[test]
    fn send_failure_restarts_and_retries_same_cue_without_duplication() {
        let (mut supervisor, state) = fake_supervisor();
        supervisor.prewarm().expect("prewarm");
        state.lock().expect("state").play_failures = 1;
        supervisor.play(CueSound::Open).expect("retry play");
        let state = state.lock().expect("state");
        assert_eq!(state.spawn_count, 2);
        assert_eq!(state.played, vec![CueSound::Open]);
    }

    #[test]
    fn user_cue_bypasses_prewarm_failure_backoff() {
        let (mut supervisor, state) = fake_supervisor();
        state.lock().expect("state").spawn_failures = 1;
        assert!(supervisor.prewarm().is_err());
        supervisor
            .play(CueSound::Open)
            .expect("user action should retry immediately");
        let state = state.lock().expect("state");
        assert_eq!(state.spawn_count, 2);
        assert_eq!(state.played, vec![CueSound::Open]);
    }
}
