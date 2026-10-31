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
/// property, e.g. `{"event":"app_launched","launch_mode":"manual"}`. The
/// webview sends the same shape through `track`, which accepts only the
/// variants it lists.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum AnalyticsEvent {
    /// Every launch, with how the previous run ended (plan section 5). Also
    /// updates the PostHog person (`$set`, `$set_once`; see `enqueue`). A new
    /// install's first launch has `previous_exit = first_run`.
    AppLaunched {
        launch_mode: LaunchMode,
        previous_exit: PreviousExit,
        /// Only when the previous run left its session file behind.
        #[serde(skip_serializing_if = "Option::is_none")]
        previous_phase: Option<SessionPhase>,
        #[serde(skip_serializing_if = "Option::is_none")]
        previous_uptime_min: Option<u64>,
    },
    /// The frontend showed an onboarding step (through `track`). Time on a
    /// step is the gap to the next event's timestamp.
    OnboardingStep { step: OnboardingStep },
    /// Onboarding finished or skipped (through `track`).
    OnboardingFinished { outcome: OnboardingOutcome },
    /// The user accepted the current disclosure: finished onboarding with
    /// sharing on, or picked Keep sharing on the privacy check (through
    /// `track`). A no is never sent.
    ConsentGiven { surface: ConsentSurface },
    /// "Where did you hear about VoiceWave?" on the onboarding done step
    /// (through `track`). Also stored on the person, see `enqueue`.
    AcquisitionAnswered { answer: Acquisition },
    /// The first successful dictation of each UTC day. A person's first one
    /// is the install's first dictation.
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
    /// The reason picked in "Something wrong?" (plan 8.1, through `track`),
    /// at most 10 a day (see `day.rs`). Never any text.
    FeedbackReported {
        kind: FeedbackKind,
        surface: FeedbackSurface,
    },
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchMode {
    /// Started by the "Start with Windows" Run-key entry.
    Autostart,
    Manual,
}

/// How the previous run ended, from its session file (plan section 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
/// percentiles come from the day's samples, which never leave the PC. Also
/// the day file's counters (`day.rs`), where the derived fields stay `None`
/// until the summary is sent.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(default)]
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

/// Where `consent_given` was answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsentSurface {
    Onboarding,
    PrivacyCheck,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadOutcome {
    Installed,
    Failed,
    Cancelled,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HotkeyFailureKind {
    OtherInstance,
    HookRejected,
    HookThread,
    Other,
}

/// The "Something wrong?" reasons, in the order the picker lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackKind {
    WrongWords,
    MissedWords,
    TooSlow,
    TextDidNotAppear,
    WrongPlace,
    FrozeOrCrashed,
    Hotkey,
    Other,
}

/// Where "Something wrong?" was opened: the pill's quick menu, or the main
/// window's Help.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackSurface {
    Pill,
    Settings,
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
