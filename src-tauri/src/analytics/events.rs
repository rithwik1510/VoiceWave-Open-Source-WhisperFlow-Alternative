//! Every analytics event and its properties (plan 024 section 4). This enum is
//! the only way anything reaches PostHog, so its fields are the whole privacy
//! surface: enums from fixed lists, numbers and booleans only (section 2).
//! Never add a free `String` field.

use serde::{Deserialize, Serialize};

/// Serialized with the variant name as `event` and each field as an event
/// property, e.g. `{"event":"app_launched","launch_mode":"manual"}`.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum AnalyticsEvent {
    /// The first launch of an install, sent once. `estimated` marks an
    /// install that predates 0.7.7 (its first-seen time is a backfill).
    AppInstalled { estimated: bool },
    /// Every launch. Also updates the PostHog person (see `sets_person`).
    AppLaunched { launch_mode: LaunchMode },
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
}

impl AnalyticsEvent {
    /// Events that carry `$set` / `$set_once`, so a PostHog person shows the
    /// install's current setup (plan 3.3).
    pub(super) fn sets_person(&self) -> bool {
        matches!(self, Self::AppLaunched { .. } | Self::ConsentAnswered { .. })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchMode {
    /// Started by the "Start with Windows" Run-key entry.
    Autostart,
    Manual,
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
        }
    }
}
