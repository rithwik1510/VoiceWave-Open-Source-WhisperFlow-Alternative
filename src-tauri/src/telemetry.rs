//! Anonymous usage facts for the frontend to send (plan 022). Rust never
//! talks to the network for telemetry: it queues closed-set enums here and
//! `src/lib/telemetry.ts` sends them, behind the same consent gate as every
//! other signal. Never put messages, paths, device names or text in an event.
//!
//! `record` works from any thread without an `AppHandle`. Events recorded
//! before the frontend's listener exists wait in the queue for its startup
//! `drain_telemetry`; panics wait in `panics.log` (see `support`) because the
//! process may not survive them.

use serde::Serialize;
use std::collections::VecDeque;
use std::sync::{Mutex, PoisonError};

/// Oldest events are dropped past this: the frontend dedupes per day anyway.
const QUEUE_CAP: usize = 64;
#[cfg(feature = "desktop")]
const PENDING_EVENT: &str = "voicewave://telemetry-pending";

static QUEUE: Mutex<VecDeque<TelemetryEvent>> = Mutex::new(VecDeque::new());
#[cfg(feature = "desktop")]
static APP: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum TelemetryEvent {
    DictationFailed {
        kind: DictationFailureKind,
    },
    WorkerCrashed {
        worker: WorkerKind,
        exit: WorkerExit,
    },
    Panicked {
        area: PanicArea,
    },
}

/// Why a dictation failed. User cancels, "too short" and sessions replaced
/// by a newer dictation are never recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DictationFailureKind {
    /// No speech model installed.
    Setup,
    MicMissing,
    MicOpen,
    /// The speech engine failed to start or warm up.
    EngineStart,
    /// The decode deadline was missed.
    Timeout,
    /// Any other decode failure.
    Transcription,
    /// The decode finished without a final transcript.
    EmptyResult,
    /// Insertion failed (an elevated target's clipboard-only result succeeds).
    Insert,
    /// Nothing heard, or a low-confidence result treated as nothing heard.
    NoSpeech,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkerKind {
    AsrGpu,
    AsrCpu,
    FasterWhisper,
    Polish,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkerExit {
    /// Exited (or its pipe broke) mid-request.
    Crash,
    /// Missed a response deadline and was killed.
    Timeout,
    /// Never came up: executable or runtime missing, launch failed, or no
    /// ready line.
    SpawnFailed,
}

/// Top-level module of the panicking source file; `Dependency` for anything
/// outside this crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PanicArea {
    State,
    Inference,
    Audio,
    Insertion,
    Hotkey,
    Models,
    Settings,
    Other,
    Dependency,
}

impl PanicArea {
    const ALL: [PanicArea; 9] = [
        PanicArea::State,
        PanicArea::Inference,
        PanicArea::Audio,
        PanicArea::Insertion,
        PanicArea::Hotkey,
        PanicArea::Models,
        PanicArea::Settings,
        PanicArea::Other,
        PanicArea::Dependency,
    ];

    /// The serde value; also the line format of `panics.log`.
    pub fn as_str(self) -> &'static str {
        match self {
            PanicArea::State => "state",
            PanicArea::Inference => "inference",
            PanicArea::Audio => "audio",
            PanicArea::Insertion => "insertion",
            PanicArea::Hotkey => "hotkey",
            PanicArea::Models => "models",
            PanicArea::Settings => "settings",
            PanicArea::Other => "other",
            PanicArea::Dependency => "dependency",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|area| area.as_str() == value)
    }

    /// Maps a panic location's file. Our own files are crate-relative
    /// (`src\state.rs`, `src/inference/asr_worker.rs`); dependencies and std
    /// carry absolute registry or `/rustc/...` paths.
    pub fn from_file(file: &str) -> Self {
        let file = file.replace('\\', "/");
        let Some(own) = file.strip_prefix("src/") else {
            return PanicArea::Dependency;
        };
        let module = own.split('/').next().unwrap_or_default();
        let module = module.strip_suffix(".rs").unwrap_or(module);
        match module {
            m if m.starts_with("state") => PanicArea::State,
            "inference" => PanicArea::Inference,
            "audio" => PanicArea::Audio,
            "insertion" => PanicArea::Insertion,
            "hotkey" => PanicArea::Hotkey,
            "model_manager" => PanicArea::Models,
            "settings" => PanicArea::Settings,
            _ => PanicArea::Other,
        }
    }
}

/// Lets `record` nudge the frontend from now on. Called once from setup.
#[cfg(feature = "desktop")]
pub fn attach(app: tauri::AppHandle) {
    let _ = APP.set(app);
}

/// Queues `event` and, once the app handle is attached, emits
/// `voicewave://telemetry-pending` so the frontend drains it.
pub fn record(event: TelemetryEvent) {
    {
        let mut queue = QUEUE.lock().unwrap_or_else(PoisonError::into_inner);
        if queue.len() >= QUEUE_CAP {
            queue.pop_front();
        }
        queue.push_back(event);
    }
    tracing::debug!("telemetry queued: {event:?}");
    #[cfg(feature = "desktop")]
    if let Some(app) = APP.get() {
        use tauri::Emitter;
        let _ = app.emit(PENDING_EVENT, ());
    }
}

/// Returns and clears the queue and the panic file (panics from this run and
/// earlier ones). Called by the frontend after startup and on every nudge.
#[cfg_attr(feature = "desktop", tauri::command)]
pub fn drain_telemetry() -> Vec<TelemetryEvent> {
    let mut events: Vec<TelemetryEvent> = crate::support::take_panic_areas()
        .into_iter()
        .map(|area| TelemetryEvent::Panicked { area })
        .collect();
    events.extend(
        QUEUE
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain(..),
    );
    events
}
