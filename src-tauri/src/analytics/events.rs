//! Every analytics event and its properties (plan 024 section 4). This enum is
//! the only way anything reaches PostHog, so its fields are the whole privacy
//! surface: enums from fixed lists, numbers and booleans only (section 2).
//! Never add a free `String` field.

use super::context::ModelFamily;
use crate::insertion::AppKind;
use crate::model_manager::DownloadFailureStage;
use crate::settings::PolishProfile;
use crate::telemetry::{DictationFailureKind, PanicArea, WorkerCause, WorkerExit, WorkerKind};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Serialized with the variant name as `event` and each field as an event
/// property, e.g. `{"event":"app_launched","launch_mode":"manual"}`.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum AnalyticsEvent {
    /// The first launch of an install, sent once. `estimated` marks an
    /// install that predates 0.7.7 (its first-seen time is a backfill).
    AppInstalled { estimated: bool },
    /// Every launch, with how the previous run ended (plan section 5). Also
    /// updates the PostHog person (see `sets_person`).
    AppLaunched {
        launch_mode: LaunchMode,
        previous_exit: PreviousExit,
        /// Only when the previous run left its session file behind.
        #[serde(skip_serializing_if = "Option::is_none")]
        previous_phase: Option<SessionPhase>,
        #[serde(skip_serializing_if = "Option::is_none")]
        previous_uptime_min: Option<u64>,
    },
    /// The frontend reached an onboarding step (through `track`).
    OnboardingStep {
        step: OnboardingStep,
        seconds_on_previous: u32,
    },
    /// Onboarding finished or skipped (through `track`).
    OnboardingFinished {
        outcome: OnboardingOutcome,
        total_s: u32,
    },
    /// The user answered the v2 privacy check with "Keep sharing" (through
    /// `track`). Turning sharing off sends nothing.
    ConsentAnswered { choice: ConsentChoice },
    /// "Where did you hear about VoiceWave?" on the onboarding done step
    /// (through `track`). Also stored on the person, see `enqueue`.
    AcquisitionAnswered { answer: Acquisition },
    /// The install's first successful dictation, sent once.
    FirstDictation { latency_ms: u64, audio_ms: u64 },
    /// The first successful dictation of each UTC day.
    ActiveDay,
    /// One earlier UTC day's dictations (plan 4.2), timestamped that day
    /// at 23:59:59 UTC.
    DaySummary(DaySummary),
    /// At most 20 a day (see `day.rs`).
    DictationFailed {
        #[serde(serialize_with = "snake_case")]
        kind: DictationFailureKind,
        /// Only for `kind = insert`, when the foreground app was resolved.
        #[serde(skip_serializing_if = "Option::is_none")]
        app_kind: Option<AppKind>,
    },
    WorkerCrashed {
        #[serde(serialize_with = "snake_case")]
        worker: WorkerKind,
        #[serde(serialize_with = "snake_case")]
        exit: WorkerExit,
        cause: WorkerCause,
    },
    /// A Rust panic, reported from `panics.log` (normally on the next launch).
    AppError {
        #[serde(serialize_with = "snake_case")]
        area: PanicArea,
    },
    /// A dictation ran on the CPU instead of the GPU.
    SpeechFallback { reason: FallbackReason },
    ModelDownloadFinished {
        model: ModelFamily,
        outcome: DownloadOutcome,
        #[serde(
            serialize_with = "snake_case",
            skip_serializing_if = "Option::is_none"
        )]
        failure_stage: Option<DownloadFailureStage>,
        retries: u32,
        downloaded_pct: u8,
        duration_s: u64,
    },
    UpdateFailed {
        stage: UpdateStage,
        /// Not set for `not_applied`, which is found at the next launch.
        #[serde(skip_serializing_if = "Option::is_none")]
        error_kind: Option<UpdateErrorKind>,
        target_version: Semver,
    },
    HotkeyFailed { kind: HotkeyFailureKind },
}

impl AnalyticsEvent {
    /// Events that carry `$set` / `$set_once`, so a PostHog person shows the
    /// install's current setup (plan 3.3).
    pub(super) fn sets_person(&self) -> bool {
        matches!(self, Self::AppLaunched { .. } | Self::ConsentAnswered { .. })
    }
}

/// The plan's values are snake_case; the TelemetryDeck enums reused here
/// serialize kebab-case or camelCase for the frontend, so their serde name is
/// converted (`mic-missing` → `mic_missing`, `diskFull` → `disk_full`).
/// Also used through `Option`, whose `None` is always skipped.
fn snake_case<T: Serialize, S: Serializer>(value: &T, serializer: S) -> Result<S::Ok, S::Error> {
    let name = match serde_json::to_value(value) {
        Ok(serde_json::Value::String(name)) => name,
        _ => "other".to_string(),
    };
    let mut snake = String::with_capacity(name.len() + 4);
    for c in name.chars() {
        match c {
            '-' => snake.push('_'),
            c if c.is_ascii_uppercase() => {
                snake.push('_');
                snake.push(c.to_ascii_lowercase());
            }
            c => snake.push(c),
        }
    }
    serializer.serialize_str(&snake)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchMode {
    /// Started by the "Start with Windows" Run-key entry.
    Autostart,
    Manual,
}

/// How the previous run ended, from its session file (plan section 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviousExit {
    FirstRun,
    Clean,
    Crash,
    AfterPanic,
    SystemRestart,
    /// The previous run was installing an update, and this is a newer version.
    Update,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionPhase {
    Startup,
    Idle,
    Dictating,
    Updating,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnboardingStep {
    Welcome,
    Mic,
    Speak,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnboardingOutcome {
    Finished,
    Skipped,
}

/// `day_summary` properties (plan 4.2). Counts are for the UTC day; the
/// percentiles come from the day's samples, which never leave the PC.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct DaySummary {
    /// Dictations started (a Try again is counted in `retried` instead).
    pub dictations: u32,
    pub succeeded: u32,
    /// No text came back (`empty_result`); also counted in `failed`.
    pub empty: u32,
    /// Every failure kind except `no_speech`.
    pub failed: u32,
    pub retried: u32,
    pub no_speech: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_p50_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_p90_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_p50_ms: Option<u64>,
    pub words_total: u64,
    /// Of the dictations that reached insertion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu_share_pct: Option<u8>,
    pub hands_free: u32,
    pub insert_typed: u32,
    pub insert_pasted: u32,
    pub insert_clipboard_only: u32,
    pub insert_history_only: u32,
    pub insert_blocked_elevated: u32,
    pub app_browser: u32,
    pub app_code_editor: u32,
    pub app_terminal: u32,
    pub app_chat: u32,
    pub app_office: u32,
    pub app_notes: u32,
    pub app_ai: u32,
    pub app_other: u32,
    pub app_unknown: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format_style: Option<PolishProfile>,
}

/// Why a dictation stepped down from the GPU, bucketed from the speech
/// worker's `asr_step_down_reason` (see `FallbackReason::classify`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FallbackReason {
    Preparing,
    Cooldown,
    DriverDisabled,
    Unavailable,
    Fault,
}

impl FallbackReason {
    /// The same buckets as the frontend's `classifyGpuStepDown`
    /// (src/lib/telemetry.ts): prefixes from `primary_rung_blocking` in
    /// inference/asr_worker.rs. A GPU fault the CPU retried carries free
    /// error text, so `backend_fallback` identifies it. `gpu-retry-recovered`
    /// (a fresh GPU worker finished the dictation) is not a step-down.
    pub fn classify(reason: &str, backend_fallback: bool) -> Option<Self> {
        if reason.starts_with("gpu-retry-recovered") {
            None
        } else if reason.starts_with("gpu-cooldown") {
            Some(Self::Cooldown)
        } else if reason.starts_with("driver-disabled") {
            Some(Self::DriverDisabled)
        } else if reason.starts_with("gpu-preparing") {
            Some(Self::Preparing)
        } else if backend_fallback {
            Some(Self::Fault)
        } else {
            Some(Self::Unavailable)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadOutcome {
    Installed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsentChoice {
    KeepSharing,
}

/// The acquisition choices, in the order the question lists them (plan
/// section 7). Fixed at release: PostHog charts group by these values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Acquisition {
    Reddit,
    X,
    Youtube,
    Github,
    Search,
    AiAssistant,
    HackerNews,
    #[serde(rename = "alternativeto")]
    AlternativeTo,
    #[serde(rename = "saashub")]
    SaasHub,
    #[serde(rename = "devhunt")]
    DevHunt,
    Friend,
    Winget,
    Article,
    Other,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateStage {
    Download,
    Verify,
    Install,
    /// The installer ran but the next launch is still the old version.
    NotApplied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateErrorKind {
    Network,
    Signature,
    Disk,
    Permission,
    Other,
}

/// Why the keyboard hook couldn't be set up (see `HotkeyError::failure_kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HotkeyFailureKind {
    OtherInstance,
    HookRejected,
    HookThread,
    Other,
}

/// A plain `major.minor.patch` version, the only version string the plan
/// allows (section 2); anything else is sent as `other`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Semver(Option<(u32, u32, u32)>);

impl Semver {
    pub fn parse(value: &str) -> Self {
        let mut parts = value.trim().split('.').map(|part| {
            (!part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
                .then(|| part.parse::<u32>().ok())
                .flatten()
        });
        let parsed = match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(Some(major)), Some(Some(minor)), Some(Some(patch)), None) => {
                Some((major, minor, patch))
            }
            _ => None,
        };
        Self(parsed)
    }

    pub fn current() -> Self {
        Self::parse(env!("CARGO_PKG_VERSION"))
    }

    pub fn is_valid(self) -> bool {
        self.0.is_some()
    }
}

impl Serialize for Semver {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Some((major, minor, patch)) => {
                serializer.serialize_str(&format!("{major}.{minor}.{patch}"))
            }
            None => serializer.serialize_str("other"),
        }
    }
}

/// Never fails: a string that isn't plain semver becomes `other`.
impl<'de> Deserialize<'de> for Semver {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Ok(Self::parse(&value))
    }
}

/// The events the webview may send through the `track` command, a subset of
/// `AnalyticsEvent`. An unknown `type` or value fails to deserialize, so the
/// frontend can't invent events or values. Mirrored by `ClientEvent` in
/// src/types/voicewave.ts.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ClientEvent {
    OnboardingStep {
        step: OnboardingStep,
        seconds_on_previous: u32,
    },
    OnboardingFinished {
        outcome: OnboardingOutcome,
        total_s: u32,
    },
    ConsentAnswered {
        choice: ConsentChoice,
    },
    AcquisitionAnswered {
        answer: Acquisition,
    },
    /// The in-app updater threw (src/lib/updater.ts).
    UpdateFailed {
        stage: UpdateStage,
        error_kind: UpdateErrorKind,
        target_version: Semver,
    },
}

impl From<ClientEvent> for AnalyticsEvent {
    fn from(event: ClientEvent) -> Self {
        match event {
            ClientEvent::OnboardingStep {
                step,
                seconds_on_previous,
            } => Self::OnboardingStep {
                step,
                seconds_on_previous,
            },
            ClientEvent::OnboardingFinished { outcome, total_s } => {
                Self::OnboardingFinished { outcome, total_s }
            }
            ClientEvent::ConsentAnswered { choice } => Self::ConsentAnswered { choice },
            ClientEvent::AcquisitionAnswered { answer } => Self::AcquisitionAnswered { answer },
            ClientEvent::UpdateFailed {
                stage,
                error_kind,
                target_version,
            } => Self::UpdateFailed {
                stage,
                error_kind: Some(error_kind),
                target_version,
            },
        }
    }
}
