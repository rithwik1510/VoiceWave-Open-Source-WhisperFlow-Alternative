//! Dictation's microphone stream, kept open between presses with a short
//! in-memory pre-roll.
//!
//! Opening the mic on every press lost the start of the first word: WASAPI
//! stream setup plus the device's own start-up meant speech from the first
//! 50-300 ms after the hotkey never reached us. One thread here owns the
//! capture stream:
//!
//! - The device and its format are resolved ahead of time. That does not
//!   start a capture session, so the Windows mic indicator stays off, and a
//!   cold press only pays stream Initialize + Start.
//! - After each dictation the stream keeps running for `WARM_WINDOW` (or for
//!   as long as the app runs, with "Keep mic ready"), filling a `PREROLL_MS`
//!   ring. A press inside the window starts its recording with that ring, so
//!   speech that began just before the press is kept.
//! - When the window lapses the stream closes (mic indicator off) and the
//!   ring is dropped. The ring lives in memory only and is never written
//!   anywhere.

use super::{
    build_input_stream, downmix_to_mono, is_likely_low_quality_asr_input, resample_linear,
    AudioCaptureService, AudioError, CaptureOptions, TARGET_SAMPLE_RATE,
};
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Stream, StreamConfig, SupportedStreamConfig,
};
use std::{
    collections::VecDeque,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{
        mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender},
        Arc, Mutex, MutexGuard,
    },
    time::{Duration, Instant},
};

/// How long the stream stays open after a dictation. Dictation comes in
/// bursts (reply, read, reply again) and most follow-ups land within a
/// minute or two; much longer and the mic-in-use icon lingers well after the
/// user has moved on.
const WARM_WINDOW: Duration = Duration::from_secs(120);
/// Audio kept from before the press. Covers starting to speak as the keys go
/// down plus a soft onset's lead-in, and stays short enough that the end of a
/// conversation you just finished rarely leaks into the dictation. The
/// whole-capture edge trim keeps 500 ms before the first voiced frame, so
/// leading silence beyond that is trimmed exactly as before.
const PREROLL_MS: usize = 600;
const PREROLL_SAMPLES: usize = TARGET_SAMPLE_RATE as usize * PREROLL_MS / 1000;
/// Idle health check cadence while the stream is open.
const HEALTH_TICK: Duration = Duration::from_secs(2);
/// No callback for this long means the stream died quietly (sleep/resume,
/// driver reset), so it is reopened instead of trusted.
const STALL_LIMIT: Duration = Duration::from_millis(1_500);
const REOPEN_BACKOFF_MIN: Duration = Duration::from_secs(2);
const REOPEN_BACKOFF_MAX: Duration = Duration::from_secs(60);
/// Upper bound on waiting for the keeper thread to open the mic for a press.
const BEGIN_TIMEOUT: Duration = Duration::from_secs(10);

/// Handle to the keeper thread. Cheap to clone.
#[derive(Clone)]
pub struct MicKeeper {
    commands: Sender<Command>,
}

impl MicKeeper {
    pub fn spawn(audio: AudioCaptureService, device: Option<String>, keep_ready: bool) -> Self {
        let (commands, receiver) = mpsc::channel();
        let for_thread = commands.clone();
        let spawned = std::thread::Builder::new()
            .name("voicewave-mic".to_string())
            .spawn(move || Keeper::new(audio, for_thread, device, keep_ready).run(receiver));
        if let Err(err) = spawned {
            // Every press then fails with "microphone service stopped".
            tracing::error!("mic keeper thread failed to start: {err}");
        }
        Self { commands }
    }

    /// Apply the Settings choices. Unchanged values are a no-op.
    pub fn configure(&self, device: Option<String>, keep_ready: bool) {
        let _ = self
            .commands
            .send(Command::Configure { device, keep_ready });
    }

    /// Record one dictation from the kept stream, opening it if needed. Same
    /// contract as `AudioCaptureService::capture_segments_from_microphone_with_signals_and_observer`;
    /// `pressed_at` only feeds the start-latency log line.
    #[allow(clippy::too_many_arguments)]
    pub fn capture<F, G, H, I>(
        &self,
        audio: &AudioCaptureService,
        device: Option<&str>,
        options: CaptureOptions,
        pressed_at: Instant,
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
        let (reply, replied) = mpsc::sync_channel(1);
        self.commands
            .send(Command::Begin {
                device: device.map(str::to_string),
                reply,
            })
            .map_err(|_| keeper_stopped())?;
        let begun = replied
            .recv_timeout(BEGIN_TIMEOUT)
            .map_err(|_| keeper_stopped())??;
        let _end = EndOnDrop {
            commands: self.commands.clone(),
            capture_id: begun.capture_id,
        };
        on_stream_ready();

        // The pre-roll arrives as the first chunk; the log line marks the
        // first live chunk after it.
        let preroll_used = begun.preroll_samples > 0;
        let preroll_ms = begun.preroll_samples * 1000 / TARGET_SAMPLE_RATE as usize;
        let mut chunks_before_live = usize::from(preroll_used);
        let mut live_logged = false;
        let segments = audio.collect_segments_from_stream(
            TARGET_SAMPLE_RATE,
            1,
            begun.audio_rx,
            begun.error_rx,
            options,
            should_cancel,
            should_stop,
            |chunk, voiced| {
                if !live_logged {
                    if chunks_before_live > 0 {
                        chunks_before_live -= 1;
                    } else {
                        live_logged = true;
                        tracing::info!(
                            "mic capture started: stream={}, open_ms={}, press_to_live_audio_ms={}, preroll_used={preroll_used}, preroll_ms={preroll_ms}",
                            if begun.open_ms.is_some() { "cold" } else { "warm" },
                            begun.open_ms.unwrap_or(0),
                            pressed_at.elapsed().as_millis(),
                        );
                    }
                }
                on_normalized_chunk(chunk, voiced);
            },
        )?;
        if segments.is_empty() {
            return Err(AudioError::NoSpeechDetected);
        }
        Ok(segments)
    }
}

fn keeper_stopped() -> AudioError {
    AudioError::RuntimeStream("microphone service stopped".to_string())
}

/// Tells the keeper the capture is over on every exit path (finish, cancel,
/// error, panic), which starts the warm window.
struct EndOnDrop {
    commands: Sender<Command>,
    capture_id: u64,
}

impl Drop for EndOnDrop {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::End {
            capture_id: self.capture_id,
        });
    }
}

enum Command {
    Configure {
        device: Option<String>,
        keep_ready: bool,
    },
    Begin {
        device: Option<String>,
        reply: SyncSender<Result<Begun, AudioError>>,
    },
    End {
        capture_id: u64,
    },
    StreamFailed {
        generation: u64,
        error: String,
    },
}

struct Begun {
    capture_id: u64,
    audio_rx: Receiver<Vec<f32>>,
    error_rx: Receiver<String>,
    preroll_samples: usize,
    /// Time spent opening the stream; None when it was already running.
    open_ms: Option<u64>,
}

/// State shared with the stream callbacks.
#[derive(Default)]
struct Shared {
    /// Latest audio at 16 kHz mono, at most PREROLL_SAMPLES long.
    ring: VecDeque<f32>,
    /// The running capture; while set, chunks go here instead of the ring.
    tap: Option<Tap>,
    last_callback_at: Option<Instant>,
}

struct Tap {
    audio_tx: Sender<Vec<f32>>,
    error_tx: Sender<String>,
}

impl Shared {
    fn push(&mut self, chunk: Vec<f32>) {
        self.last_callback_at = Some(Instant::now());
        let chunk = match &self.tap {
            Some(tap) => match tap.audio_tx.send(chunk) {
                Ok(()) => return,
                // The capture finished and dropped its receiver.
                Err(mpsc::SendError(chunk)) => {
                    self.tap = None;
                    chunk
                }
            },
            None => chunk,
        };
        self.ring.extend(chunk);
        let excess = self.ring.len().saturating_sub(PREROLL_SAMPLES);
        self.ring.drain(..excess);
    }
}

fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    shared
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// cpal panics on some vanished-device paths (`Device::name`); unguarded,
/// that would kill the keeper thread for the rest of the session.
fn guarded<T>(call: impl FnOnce() -> Result<T, AudioError>) -> Result<T, AudioError> {
    catch_unwind(AssertUnwindSafe(call)).unwrap_or_else(|_| {
        Err(AudioError::RuntimeStream(
            "audio device call panicked".to_string(),
        ))
    })
}

fn default_input_name() -> Option<String> {
    guarded(|| {
        Ok(cpal::default_host()
            .default_input_device()
            .and_then(|device| device.name().ok()))
    })
    .ok()
    .flatten()
}

fn input_device_present(name: &str) -> bool {
    guarded(|| {
        let mut devices = cpal::default_host()
            .input_devices()
            .map_err(AudioError::Devices)?;
        Ok(devices.any(|device| device.name().is_ok_and(|found| found == name)))
    })
    .unwrap_or(false)
}

/// A device resolved for the current Settings choice, format included.
struct Prepared {
    requested: Option<String>,
    device: cpal::Device,
    name: String,
    config: SupportedStreamConfig,
}

struct OpenStream {
    stream: Stream,
    requested: Option<String>,
    name: String,
    generation: u64,
    opened_at: Instant,
    failed: bool,
}

struct Keeper {
    audio: AudioCaptureService,
    commands: Sender<Command>,
    shared: Arc<Mutex<Shared>>,
    device: Option<String>,
    keep_ready: bool,
    prepared: Option<Prepared>,
    open: Option<OpenStream>,
    generation: u64,
    next_capture_id: u64,
    capturing: Option<u64>,
    warm_until: Option<Instant>,
    retry_at: Option<Instant>,
    retry_backoff: Duration,
}

impl Keeper {
    fn new(
        audio: AudioCaptureService,
        commands: Sender<Command>,
        device: Option<String>,
        keep_ready: bool,
    ) -> Self {
        Self {
            audio,
            commands,
            shared: Arc::default(),
            device,
            keep_ready,
            prepared: None,
            open: None,
            generation: 0,
            next_capture_id: 0,
            capturing: None,
            warm_until: None,
            retry_at: None,
            retry_backoff: REOPEN_BACKOFF_MIN,
        }
    }

    fn run(mut self, commands: Receiver<Command>) {
        self.settle();
        loop {
            let command = match self.next_wake() {
                Some(wait) => match commands.recv_timeout(wait) {
                    Ok(command) => Some(command),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => return,
                },
                None => match commands.recv() {
                    Ok(command) => Some(command),
                    Err(_) => return,
                },
            };
            match command {
                Some(Command::Configure { device, keep_ready }) => {
                    self.configure(device, keep_ready)
                }
                Some(Command::Begin { device, reply }) => self.begin(device, reply),
                Some(Command::End { capture_id }) => self.end(capture_id),
                Some(Command::StreamFailed { generation, error }) => {
                    if let Some(open) = self.open.as_mut().filter(|o| o.generation == generation) {
                        tracing::warn!("mic stream error on '{}': {error}", open.name);
                        open.failed = true;
                    }
                    self.settle();
                }
                None => self.settle(),
            }
        }
    }

    fn next_wake(&self) -> Option<Duration> {
        // A capture always ends with an End command, which wakes us.
        if self.capturing.is_some() {
            return None;
        }
        let now = Instant::now();
        if self.open.is_some() {
            let window_left = self
                .warm_until
                .filter(|_| !self.keep_ready)
                .map(|until| until.saturating_duration_since(now));
            return Some(window_left.map_or(HEALTH_TICK, |left| left.min(HEALTH_TICK)));
        }
        if self.wants_stream() {
            return self.retry_at.map(|at| at.saturating_duration_since(now));
        }
        None
    }

    fn wants_stream(&self) -> bool {
        self.keep_ready || self.warm_until.is_some_and(|until| Instant::now() < until)
    }

    fn configure(&mut self, device: Option<String>, keep_ready: bool) {
        if device == self.device && keep_ready == self.keep_ready {
            return;
        }
        if device != self.device {
            self.prepared = None;
        }
        let turned_off = self.keep_ready && !keep_ready;
        self.device = device;
        self.keep_ready = keep_ready;
        self.retry_at = None;
        self.retry_backoff = REOPEN_BACKOFF_MIN;
        if turned_off {
            // Turning the setting off releases the mic now rather than at the
            // end of a warm window the user never asked for.
            self.warm_until = None;
            if self.capturing.is_none() {
                self.close("keep mic ready turned off");
            }
        }
        self.settle();
    }

    fn begin(&mut self, device: Option<String>, reply: SyncSender<Result<Begun, AudioError>>) {
        if device != self.device {
            self.device = device;
            self.prepared = None;
        }
        let open_ms = match self.ensure_open_for_press() {
            Ok(open_ms) => open_ms,
            Err(err) => {
                let _ = reply.send(Err(err));
                return;
            }
        };
        self.next_capture_id += 1;
        let capture_id = self.next_capture_id;
        let (audio_tx, audio_rx) = mpsc::channel();
        let (error_tx, error_rx) = mpsc::channel();
        let preroll_samples = {
            let mut shared = lock(&self.shared);
            // Queued ahead of any live chunk, under the same lock the
            // callback uses, so nothing is lost or duplicated at the seam.
            let preroll: Vec<f32> = shared.ring.drain(..).collect();
            let samples = preroll.len();
            if samples > 0 {
                let _ = audio_tx.send(preroll);
            }
            shared.tap = Some(Tap { audio_tx, error_tx });
            samples
        };
        self.capturing = Some(capture_id);
        let begun = Begun {
            capture_id,
            audio_rx,
            error_rx,
            preroll_samples,
            open_ms,
        };
        if reply.send(Ok(begun)).is_err() {
            // The press gave up waiting (BEGIN_TIMEOUT).
            self.end(capture_id);
        }
    }

    /// Returns how long opening took, or None when a healthy stream on the
    /// right device was already running.
    fn ensure_open_for_press(&mut self) -> Result<Option<u64>, AudioError> {
        if let Some(reason) = self.stale_reason() {
            self.close(reason);
        }
        if self.open.is_some() {
            return Ok(None);
        }
        let started = Instant::now();
        self.open()?;
        Ok(Some(started.elapsed().as_millis() as u64))
    }

    fn end(&mut self, capture_id: u64) {
        // A cancelled capture can finish after its replacement has begun.
        if self.capturing != Some(capture_id) {
            return;
        }
        self.capturing = None;
        lock(&self.shared).tap = None;
        let bluetooth_call_mic = self
            .open
            .as_ref()
            .is_some_and(|open| is_likely_low_quality_asr_input(&open.name));
        if bluetooth_call_mic && !self.keep_ready {
            // An open headset mic holds Bluetooth in its call profile, which
            // makes music sound bad for the whole window. Only keep it open
            // when the user explicitly chose "Keep mic ready".
            tracing::info!("mic: no warm window for a Bluetooth hands-free input");
            self.warm_until = None;
        } else {
            self.warm_until = Some(Instant::now() + WARM_WINDOW);
        }
        self.settle();
    }

    /// Bring the idle stream in line with Settings, the warm window and
    /// device health. Never touches a running capture.
    fn settle(&mut self) {
        if self.capturing.is_some() {
            return;
        }
        if let Some(reason) = self.stale_reason() {
            self.close(reason);
        }
        if self.open.is_some() && !self.wants_stream() {
            self.close("warm window over");
        }
        let retry_due = self.retry_at.is_none_or(|at| Instant::now() >= at);
        if self.open.is_none() && self.wants_stream() && retry_due {
            let _ = self.open();
        }
        if self.open.is_none() && self.prepared.is_none() {
            self.prepare();
        }
    }

    /// Why the open stream should not be used, if anything.
    fn stale_reason(&self) -> Option<&'static str> {
        let open = self.open.as_ref()?;
        if open.requested != self.device {
            return Some("input device changed in Settings");
        }
        if open.failed {
            return Some("stream error");
        }
        let last_audio = lock(&self.shared)
            .last_callback_at
            .unwrap_or(open.opened_at);
        if last_audio.elapsed() > STALL_LIMIT {
            return Some("stream stopped delivering audio");
        }
        let still_current = match open.requested.as_deref() {
            // Following the Windows default: did it change?
            None => default_input_name().is_none_or(|name| name == open.name),
            Some(requested) if requested == open.name => true,
            // Running on a fallback: is the chosen mic back?
            Some(requested) => !input_device_present(requested),
        };
        (!still_current).then_some("input device changed")
    }

    fn resolve(&self, requested: Option<&str>) -> Result<(cpal::Device, String), AudioError> {
        guarded(|| {
            let device = self
                .audio
                .select_input_device(&cpal::default_host(), requested)?;
            let name = device.name().unwrap_or_default();
            Ok((device, name))
        })
    }

    fn resolve_prepared(&self) -> Result<Prepared, AudioError> {
        let (device, name) = self.resolve(self.device.as_deref())?;
        let config = guarded(|| {
            device
                .default_input_config()
                .map_err(AudioError::DefaultInputConfig)
        })?;
        Ok(Prepared {
            requested: self.device.clone(),
            device,
            name,
            config,
        })
    }

    /// Resolve the device and format ahead of the next cold press. This does
    /// not start a capture session, so the mic indicator stays off.
    fn prepare(&mut self) {
        match self.resolve_prepared() {
            Ok(prepared) => self.prepared = Some(prepared),
            Err(err) => tracing::info!("mic prepare deferred: {err}"),
        }
    }

    /// The prepared device, if it is still what a fresh resolution would pick.
    fn take_prepared(&mut self) -> Option<Prepared> {
        let prepared = self.prepared.take()?;
        if prepared.requested != self.device {
            return None;
        }
        let current = match self.device.as_deref() {
            None => default_input_name().is_some_and(|name| name == prepared.name),
            Some(requested) => requested == prepared.name,
        };
        current.then_some(prepared)
    }

    fn open(&mut self) -> Result<(), AudioError> {
        let opened = match self.take_prepared() {
            Some(prepared) => match self.start_stream(prepared) {
                Ok(open) => Ok(open),
                Err(err) => {
                    // Format or device changed since it was prepared.
                    tracing::info!("mic: prepared device failed to open ({err}); resolving again");
                    self.resolve_prepared()
                        .and_then(|prepared| self.start_stream(prepared))
                }
            },
            None => self
                .resolve_prepared()
                .and_then(|prepared| self.start_stream(prepared)),
        };
        match opened {
            Ok(open) => {
                tracing::info!(
                    "mic stream opened on '{}' (warm window {}s, keep ready {})",
                    open.name,
                    WARM_WINDOW.as_secs(),
                    self.keep_ready
                );
                self.open = Some(open);
                self.retry_at = None;
                self.retry_backoff = REOPEN_BACKOFF_MIN;
                Ok(())
            }
            Err(err) => {
                tracing::warn!("mic stream failed to open: {err}");
                self.retry_at = Some(Instant::now() + self.retry_backoff);
                self.retry_backoff = (self.retry_backoff * 2).min(REOPEN_BACKOFF_MAX);
                Err(err)
            }
        }
    }

    fn start_stream(&mut self, prepared: Prepared) -> Result<OpenStream, AudioError> {
        let Prepared {
            requested,
            device,
            name,
            config,
        } = prepared;
        self.generation += 1;
        let generation = self.generation;
        let sample_rate = config.sample_rate().0;
        let channels = config.channels();
        let sample_format = config.sample_format();
        let stream_config: StreamConfig = config.into();
        {
            let mut shared = lock(&self.shared);
            shared.ring.clear();
            shared.last_callback_at = None;
        }
        let shared = Arc::clone(&self.shared);
        let shared_for_errors = Arc::clone(&self.shared);
        let commands = self.commands.clone();
        let stream = guarded(|| {
            let stream = build_input_stream(
                &device,
                &stream_config,
                sample_format,
                move |raw| {
                    // Same per-chunk normalization the capture loop used to
                    // do, so the ring holds ready-to-use 16 kHz mono.
                    let chunk = resample_linear(
                        &downmix_to_mono(&raw, channels),
                        sample_rate,
                        TARGET_SAMPLE_RATE,
                    );
                    lock(&shared).push(chunk);
                },
                move |error| {
                    if let Some(tap) = &lock(&shared_for_errors).tap {
                        let _ = tap.error_tx.send(error.clone());
                    }
                    let _ = commands.send(Command::StreamFailed { generation, error });
                },
            )?;
            stream.play().map_err(AudioError::PlayStream)?;
            Ok(stream)
        })?;
        Ok(OpenStream {
            stream,
            requested,
            name,
            generation,
            opened_at: Instant::now(),
            failed: false,
        })
    }

    fn close(&mut self, reason: &str) {
        let Some(open) = self.open.take() else {
            return;
        };
        // Dropping the stream joins cpal's capture thread, so no callback
        // can touch the ring after this.
        drop(open.stream);
        {
            let mut shared = lock(&self.shared);
            shared.ring = VecDeque::new();
            shared.tap = None;
            shared.last_callback_at = None;
        }
        tracing::info!("mic stream closed on '{}' ({reason})", open.name);
    }
}
