//! The recorder: one background thread that queues every `capture` and turns
//! dictation facts into the day accumulator (plan 4.2) and its events
//! (`first_dictation`, `active_day`, `day_summary`, capped `dictation_failed`,
//! `speech_fallback`). Callers only send on a channel, so nothing on the
//! dictation path waits for analytics' disk writes.
//!
//! The day is kept in `<data_dir>/analytics-day.json`: the UTC day, its
//! counts, and up to 500 latency and audio samples for the percentiles. Raw
//! samples never leave the file. With sharing off the file isn't written, and
//! an existing one is deleted.

use super::events::{AnalyticsEvent, DaySummary, FallbackReason};
use super::{identity, lock, now, Consent, CONSENT};
use crate::atomic_file::{self, StoreLoad};
use crate::insertion::AppKind;
use crate::settings::PolishProfile;
use crate::telemetry::DictationFailureKind;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{mpsc, OnceLock},
};

const DAY_MS: u64 = 24 * 60 * 60 * 1000;
const MAX_SAMPLES: usize = 500;
/// `dictation_failed` events per UTC day; the day's counts still see them all.
const MAX_FAILURE_EVENTS: u32 = 20;

/// What the dictation pipeline tells analytics. Microphone dictations only.
#[derive(Debug, Clone, Copy)]
pub enum DictationFact {
    /// A press was accepted (not a Try again).
    Started { hands_free: bool },
    /// Try again started on kept audio.
    Retried,
    /// A dictation reached insertion, whether or not its text landed.
    Finished(Finished),
    /// Recorded where TelemetryDeck's `DictationFailed` is (`telemetry::record`).
    Failed {
        kind: DictationFailureKind,
        app_kind: Option<AppKind>,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct Finished {
    /// The text landed (clipboard only counts, as in the Stats tab).
    pub success: bool,
    /// Key release to text inserted.
    pub latency_ms: u64,
    pub audio_ms: u64,
    pub words: u32,
    pub on_gpu: bool,
    pub insert: Option<InsertKind>,
    pub app_kind: Option<AppKind>,
    pub format_style: PolishProfile,
    pub fallback: Option<FallbackReason>,
}

/// How the text got in (the `insert_*` counters).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertKind {
    Typed,
    Pasted,
    ClipboardOnly,
    HistoryOnly,
    BlockedElevated,
}

enum Message {
    Capture(AnalyticsEvent),
    Fact(DictationFact),
    /// At startup and on consent changes: queue a due `day_summary`, or with
    /// sharing off drop the day and its file.
    CheckDay,
}

static RECORDER: OnceLock<mpsc::Sender<Message>> = OnceLock::new();

/// Starts the recorder thread. Called once by `analytics::start`.
pub(super) fn spawn(path: PathBuf, first_dictation_queued: bool) {
    let (sender, receiver) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("analytics-recorder".to_string())
        .spawn(move || Recorder::new(path, first_dictation_queued).run(receiver));
    match spawned {
        Ok(_) => {
            let _ = RECORDER.set(sender);
        }
        Err(err) => tracing::warn!("analytics: couldn't start the recorder: {err}"),
    }
}

fn send(message: Message) -> bool {
    RECORDER
        .get()
        .is_some_and(|recorder| recorder.send(message).is_ok())
}

/// Queues `event` from the recorder thread; false when it isn't running.
pub(super) fn send_capture(event: AnalyticsEvent) -> bool {
    send(Message::Capture(event))
}

pub(super) fn check_day() {
    send(Message::CheckDay);
}

/// Never blocks: the fact is handled on the recorder thread.
pub fn note_dictation(fact: DictationFact) {
    send(Message::Fact(fact));
}

/// The persisted day. Field names are internal; nothing here is sent as is.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct DayFile {
    /// Days since the Unix epoch, UTC.
    day: u64,
    dictations: u32,
    succeeded: u32,
    empty: u32,
    failed: u32,
    retried: u32,
    no_speech: u32,
    words_total: u64,
    hands_free: u32,
    /// Dictations that reached insertion, and of those, ran on the GPU.
    reached_insert: u32,
    on_gpu: u32,
    insert_typed: u32,
    insert_pasted: u32,
    insert_clipboard_only: u32,
    insert_history_only: u32,
    insert_blocked_elevated: u32,
    app_browser: u32,
    app_code_editor: u32,
    app_terminal: u32,
    app_chat: u32,
    app_office: u32,
    app_notes: u32,
    app_ai: u32,
    app_other: u32,
    app_unknown: u32,
    format_style: Option<PolishProfile>,
    latency_samples: Vec<u64>,
    audio_samples: Vec<u64>,
    active_day_sent: bool,
    failure_events_sent: u32,
}

impl DayFile {
    fn new(day: u64) -> Self {
        Self {
            day,
            ..Self::default()
        }
    }

    fn has_activity(&self) -> bool {
        self.dictations + self.retried + self.reached_insert + self.failed + self.no_speech > 0
    }

    fn summary(&self) -> DaySummary {
        DaySummary {
            dictations: self.dictations,
            succeeded: self.succeeded,
            empty: self.empty,
            failed: self.failed,
            retried: self.retried,
            no_speech: self.no_speech,
            latency_p50_ms: percentile(&self.latency_samples, 50),
            latency_p90_ms: percentile(&self.latency_samples, 90),
            audio_p50_ms: percentile(&self.audio_samples, 50),
            words_total: self.words_total,
            gpu_share_pct: (self.reached_insert > 0).then(|| {
                ((u64::from(self.on_gpu) * 100 + u64::from(self.reached_insert) / 2)
                    / u64::from(self.reached_insert)) as u8
            }),
            hands_free: self.hands_free,
            insert_typed: self.insert_typed,
            insert_pasted: self.insert_pasted,
            insert_clipboard_only: self.insert_clipboard_only,
            insert_history_only: self.insert_history_only,
            insert_blocked_elevated: self.insert_blocked_elevated,
            app_browser: self.app_browser,
            app_code_editor: self.app_code_editor,
            app_terminal: self.app_terminal,
            app_chat: self.app_chat,
            app_office: self.app_office,
            app_notes: self.app_notes,
            app_ai: self.app_ai,
            app_other: self.app_other,
            app_unknown: self.app_unknown,
            format_style: self.format_style,
        }
    }

    fn count_app(&mut self, kind: AppKind) {
        let counter = match kind {
            AppKind::Browser => &mut self.app_browser,
            AppKind::CodeEditor => &mut self.app_code_editor,
            AppKind::Terminal => &mut self.app_terminal,
            AppKind::Chat => &mut self.app_chat,
            AppKind::Office => &mut self.app_office,
            AppKind::Notes => &mut self.app_notes,
            AppKind::Ai => &mut self.app_ai,
            AppKind::Other => &mut self.app_other,
            AppKind::Unknown => &mut self.app_unknown,
        };
        *counter += 1;
    }

    fn count_insert(&mut self, kind: InsertKind) {
        let counter = match kind {
            InsertKind::Typed => &mut self.insert_typed,
            InsertKind::Pasted => &mut self.insert_pasted,
            InsertKind::ClipboardOnly => &mut self.insert_clipboard_only,
            InsertKind::HistoryOnly => &mut self.insert_history_only,
            InsertKind::BlockedElevated => &mut self.insert_blocked_elevated,
        };
        *counter += 1;
    }
}

/// Nearest-rank percentile; `None` without samples.
fn percentile(samples: &[u64], pct: usize) -> Option<u64> {
    if samples.is_empty() {
        return None;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let rank = (pct * sorted.len()).div_ceil(100).max(1);
    sorted.get(rank - 1).copied()
}

fn push_sample(samples: &mut Vec<u64>, value: u64) {
    if samples.len() < MAX_SAMPLES {
        samples.push(value);
    }
}

struct Recorder {
    path: PathBuf,
    /// Loaded on first use, once sharing is known to be on.
    day: Option<DayFile>,
    first_dictation_queued: bool,
    /// `speech_fallback` goes out once per reason per run.
    fallbacks_sent: HashSet<FallbackReason>,
}

impl Recorder {
    fn new(path: PathBuf, first_dictation_queued: bool) -> Self {
        Self {
            path,
            day: None,
            first_dictation_queued,
            fallbacks_sent: HashSet::new(),
        }
    }

    fn run(mut self, receiver: mpsc::Receiver<Message>) {
        while let Ok(first) = receiver.recv() {
            // A burst (a dictation's facts, a few captures) costs one write.
            let mut dirty = false;
            for message in std::iter::once(first).chain(receiver.try_iter()) {
                dirty |= self.handle(message);
            }
            if dirty {
                self.save();
            }
        }
    }

    /// Returns whether the day changed and needs saving.
    fn handle(&mut self, message: Message) -> bool {
        let fact = match message {
            Message::Capture(event) => {
                super::enqueue(event, None);
                return false;
            }
            Message::CheckDay => None,
            Message::Fact(fact) => Some(fact),
        };
        match *lock(&CONSENT) {
            Consent::Unknown => return false,
            Consent::Off => {
                self.drop_day();
                return false;
            }
            Consent::Hold | Consent::On => {}
        }
        let (now_ms, _) = now();
        let today = now_ms / DAY_MS;
        let mut dirty = self.roll_over(today);
        if let Some(fact) = fact {
            self.apply(fact, today);
            dirty = true;
        }
        dirty
    }

    /// Starts `today`'s file, first queueing the stored day's summary when
    /// that day is over and had dictations. A stored day in the future (the
    /// clock moved back) is dropped.
    fn roll_over(&mut self, today: u64) -> bool {
        let day = self.day.get_or_insert_with(|| load(&self.path, today));
        if day.day == today {
            return false;
        }
        if day.day < today && day.has_activity() {
            let end_of_day_ms = day.day * DAY_MS + DAY_MS - 1000;
            super::enqueue(
                AnalyticsEvent::DaySummary(day.summary()),
                Some(end_of_day_ms),
            );
        }
        *day = DayFile::new(today);
        true
    }

    fn apply(&mut self, fact: DictationFact, today: u64) {
        let day = self.day.get_or_insert_with(|| DayFile::new(today));
        match fact {
            DictationFact::Started { hands_free } => {
                day.dictations += 1;
                if hands_free {
                    day.hands_free += 1;
                }
            }
            DictationFact::Retried => day.retried += 1,
            DictationFact::Failed { kind, app_kind } => {
                match kind {
                    DictationFailureKind::NoSpeech => day.no_speech += 1,
                    DictationFailureKind::EmptyResult => {
                        day.empty += 1;
                        day.failed += 1;
                    }
                    _ => day.failed += 1,
                }
                if day.failure_events_sent < MAX_FAILURE_EVENTS
                    && super::enqueue(
                        AnalyticsEvent::DictationFailed {
                            kind,
                            app_kind: app_kind.filter(|_| kind == DictationFailureKind::Insert),
                        },
                        None,
                    )
                {
                    day.failure_events_sent += 1;
                }
            }
            DictationFact::Finished(finished) => {
                day.reached_insert += 1;
                if finished.on_gpu {
                    day.on_gpu += 1;
                }
                if let Some(kind) = finished.insert {
                    day.count_insert(kind);
                }
                if let Some(kind) = finished.app_kind {
                    day.count_app(kind);
                }
                day.format_style = Some(finished.format_style);
                if let Some(reason) = finished.fallback {
                    if self.fallbacks_sent.insert(reason) {
                        super::enqueue(AnalyticsEvent::SpeechFallback { reason }, None);
                    }
                }
                if finished.success {
                    day.succeeded += 1;
                    day.words_total += u64::from(finished.words);
                    push_sample(&mut day.latency_samples, finished.latency_ms);
                    push_sample(&mut day.audio_samples, finished.audio_ms);
                    if !day.active_day_sent && super::enqueue(AnalyticsEvent::ActiveDay, None) {
                        day.active_day_sent = true;
                    }
                    if !self.first_dictation_queued
                        && super::enqueue(
                            AnalyticsEvent::FirstDictation {
                                latency_ms: finished.latency_ms,
                                audio_ms: finished.audio_ms,
                            },
                            None,
                        )
                    {
                        self.first_dictation_queued = true;
                        identity::mark_first_dictation_queued();
                    }
                }
            }
        }
    }

    fn drop_day(&mut self) {
        self.day = None;
        for path in [
            self.path.clone(),
            atomic_file::sibling_with_suffix(&self.path, ".bak"),
            atomic_file::sibling_with_suffix(&self.path, ".tmp"),
        ] {
            if let Err(err) = std::fs::remove_file(&path) {
                if err.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!("analytics: couldn't delete the day file: {err}");
                }
            }
        }
    }

    fn save(&self) {
        let Some(day) = &self.day else {
            return;
        };
        // Sharing may have turned off since this burst was handled.
        if !lock(&CONSENT).queues() {
            return;
        }
        match serde_json::to_vec(day) {
            Ok(raw) => {
                if let Err(err) = atomic_file::atomic_write(&self.path, &raw) {
                    tracing::warn!("analytics: couldn't save the day file: {err}");
                }
            }
            Err(err) => tracing::warn!("analytics: couldn't encode the day file: {err}"),
        }
    }
}

fn load(path: &Path, today: u64) -> DayFile {
    match atomic_file::load_with_recovery(path, "analytics day", |raw| {
        serde_json::from_str::<DayFile>(raw)
    }) {
        StoreLoad::Loaded(day) | StoreLoad::Recovered(day) => day,
        StoreLoad::Missing | StoreLoad::Reset => DayFile::new(today),
    }
}
