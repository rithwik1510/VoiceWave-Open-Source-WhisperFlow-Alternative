//! Usage analytics on PostHog (plan 024), sent from Rust.
//!
//! `capture` hands an `AnalyticsEvent` to the recorder thread (`day.rs`),
//! which resolves it plus the base properties into an entry in the persisted
//! outbox; one sender thread sends the outbox to PostHog with each event's
//! real timestamp. Nothing happens without a project key (section 3.5), and
//! the consent gate (3.8) is checked when an event is queued and again before
//! each send. Quitting never waits on the network: whatever is unsent stays in
//! the outbox for the next pass or the next launch. Privacy rules: section 2
//! of the plan. The older TelemetryDeck path (`crate::telemetry`,
//! src/lib/telemetry.ts) is separate and unchanged.

mod context;
mod day;
mod events;
mod identity;
mod outbox;
mod posthog;

pub use context::{note_speech_engine, ModelFamily};
pub use day::{note_dictation, DictationFact, Finished, InsertKind};
pub use events::{
    AnalyticsEvent, DownloadOutcome, HotkeyFailureKind, LaunchMode, PreviousExit, Semver,
    SessionPhase,
};

use crate::settings::{VoiceWaveSettings, ANALYTICS_CONSENT_VERSION};
use chrono::{DateTime, Utc};
use context::{Hypervisor, OsMajor};
use events::UpdateStage;
use identity::Identity;
use outbox::{Outbox, OutboxEntry};
use posthog::SendOutcome;
use serde_json::{json, Map, Value};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Condvar, Mutex, MutexGuard, OnceLock, PoisonError,
    },
    time::Duration,
};

const OUTBOX_FILE: &str = "analytics-outbox.jsonl";
const DAY_FILE: &str = "analytics-day.json";
/// Events per `/batch/` POST.
const MAX_BATCH: usize = 50;
/// How often the sender runs without being woken: it retries a failed send
/// and checks whether the day has ended.
const IDLE_FLUSH: Duration = Duration::from_secs(60);
/// After a wake, so a burst of events goes out in one POST.
const DEBOUNCE: Duration = Duration::from_secs(2);
const DAY_MS: u64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Consent {
    /// Nothing is recorded or sent, and nothing is deleted: settings aren't
    /// loaded yet, or the user shared under an older disclosure (version 1)
    /// and hasn't answered the privacy check for the current one.
    Unknown,
    /// Sharing off: nothing is recorded, and the outbox and day file are
    /// deleted.
    Off,
    /// A new install that hasn't finished onboarding (version 0): record, but
    /// send only once onboarding has shown the current disclosure.
    Hold,
    On,
}

impl Consent {
    fn queues(self) -> bool {
        matches!(self, Self::Hold | Self::On)
    }
}

static CONSENT: Mutex<Consent> = Mutex::new(Consent::Unknown);
static CORE: OnceLock<Core> = OnceLock::new();
/// A rejected batch (bad key, too large) is logged once per run.
static REJECTION_LOGGED: AtomicBool = AtomicBool::new(false);

struct Core {
    api_key: String,
    identity: Identity,
    os_major: OsMajor,
    hypervisor: Hypervisor,
    outbox: Mutex<Outbox>,
    /// Built on the first send, off the startup path: reading the Windows
    /// certificate store for the TLS config can take a noticeable moment.
    agent: OnceLock<ureq::Agent>,
    woken: Mutex<bool>,
    wake_signal: Condvar,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn now_ms() -> u64 {
    u64::try_from(Utc::now().timestamp_millis()).unwrap_or_default()
}

/// Release builds use the key compiled in by release.yml. Debug builds send
/// nothing unless `VOICEWAVE_POSTHOG_DEV=1` is set at runtime; then they use
/// the compiled key or, failing that, the runtime `VOICEWAVE_POSTHOG_KEY`,
/// and every event carries `dev_build: true`.
fn project_key() -> Option<String> {
    let compiled = option_env!("VOICEWAVE_POSTHOG_KEY").map(str::to_string);
    let key = if cfg!(debug_assertions) {
        if std::env::var("VOICEWAVE_POSTHOG_DEV").as_deref() != Ok("1") {
            return None;
        }
        compiled.or_else(|| std::env::var("VOICEWAVE_POSTHOG_KEY").ok())
    } else {
        compiled
    };
    key.map(|key| key.trim().to_string())
        .filter(|key| !key.is_empty())
}

/// Starts analytics for this run. Call first thing in setup: it records
/// whether settings existed before anything could save them. Without a
/// project key it does nothing else, and `capture` stays a no-op.
pub fn init() {
    identity::note_settings_existed_at_launch();
    let Some(api_key) = project_key() else {
        tracing::info!("analytics: off in this build (no project key)");
        return;
    };
    let Some(data_dir) = directories::ProjectDirs::from("com", "voicewave", "localcore")
        .map(|dirs| dirs.data_dir().to_path_buf())
    else {
        return;
    };
    let Some(identity) = identity::load_or_create(now_ms()) else {
        return;
    };
    start(api_key, identity, data_dir);
}

fn start(api_key: String, identity: Identity, data_dir: PathBuf) {
    let core = Core {
        api_key,
        identity,
        os_major: OsMajor::current(),
        hypervisor: Hypervisor::detect(),
        outbox: Mutex::new(Outbox::load(data_dir.join(OUTBOX_FILE), now_ms())),
        agent: OnceLock::new(),
        woken: Mutex::new(false),
        wake_signal: Condvar::new(),
    };
    if CORE.set(core).is_err() {
        return;
    }
    let Some(core) = CORE.get() else {
        return;
    };
    if *lock(&CONSENT) == Consent::Off {
        lock(&core.outbox).purge();
    }
    day::spawn(data_dir.join(DAY_FILE));
    if let Err(err) = std::thread::Builder::new()
        .name("analytics-sender".to_string())
        .spawn(move || run_sender(core))
    {
        tracing::warn!("analytics: couldn't start the sender: {err}");
    }
}

/// Queues `event` when sharing allows it. Never blocks: the outbox write
/// happens on the recorder thread.
pub fn capture(event: AnalyticsEvent) {
    if !day::send_capture(event) {
        enqueue(event, None);
    }
}

/// Returns whether the event was queued. `at` backdates it (a day's summary,
/// the launch); otherwise it happened now.
fn enqueue(event: AnalyticsEvent, at: Option<DateTime<Utc>>) -> bool {
    let Some(core) = CORE.get() else {
        return false;
    };
    let Ok(Value::Object(mut properties)) = serde_json::to_value(event) else {
        return false;
    };
    let Some(Value::String(name)) = properties.remove("event") else {
        return false;
    };
    let now = Utc::now();
    let queued_ms = u64::try_from(now.timestamp_millis()).unwrap_or_default();
    let base = core.base_properties(queued_ms);
    if let AnalyticsEvent::AppLaunched { .. } = event {
        properties.insert("$set".to_string(), core.person_properties(&base));
        properties.insert(
            "$set_once".to_string(),
            json!({
                "first_version": Semver::parse(&core.identity.first_seen_version),
                "first_install_source": core.identity.install_source,
                "first_seen_estimated": core.identity.first_seen_estimated,
            }),
        );
    }
    if let AnalyticsEvent::AcquisitionAnswered { answer } = event {
        properties.insert("$set_once".to_string(), json!({ "acquisition": answer }));
    }
    properties.extend(base);
    let entry = OutboxEntry {
        uuid: uuid::Uuid::new_v4().to_string(),
        event: name,
        timestamp: at
            .unwrap_or(now)
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        queued_ms,
        properties,
    };
    {
        // Consent is checked under the outbox lock, so a switch-off can't
        // purge between the check and the append and leave this entry
        // behind: `set_consent` purges after it has set `Off`.
        let mut outbox = lock(&core.outbox);
        if !lock(&CONSENT).queues() {
            return false;
        }
        outbox.push(entry);
    }
    core.wake();
    true
}

/// Queues the startup events, timestamped `launched_at`: an update that
/// didn't land, then `app_launched` with how the previous run ended. Also
/// queues a due `day_summary`. Call after settings have loaded.
pub fn capture_launch(
    launch_mode: LaunchMode,
    previous: &crate::session::PreviousRun,
    launched_at: DateTime<Utc>,
) {
    let Some(core) = CORE.get() else {
        return;
    };
    if let Some(target_version) = previous.update_not_applied {
        enqueue(
            AnalyticsEvent::UpdateFailed {
                stage: UpdateStage::NotApplied,
                error_kind: None,
                target_version,
            },
            Some(launched_at),
        );
    }
    // No session file: a clean exit deleted it, or this install never ran.
    let previous_exit = match previous.exit {
        Some(exit) => exit,
        None if core.identity.first_launch => PreviousExit::FirstRun,
        None => PreviousExit::Clean,
    };
    enqueue(
        AnalyticsEvent::AppLaunched {
            launch_mode,
            previous_exit,
            previous_phase: previous.phase,
            previous_uptime_min: previous.uptime_min,
        },
        Some(launched_at),
    );
    day::check_day();
}

/// A first install of 0.7.7 or later, not an older install that updated
/// (plan 3.2). Without a project key the identity isn't loaded, so this falls
/// back to whether settings existed when this launch started.
pub fn is_new_install() -> bool {
    CORE.get().map_or_else(
        || !identity::note_settings_existed_at_launch(),
        |core| !core.identity.first_seen_estimated,
    )
}

/// Applies the consent gate (plan 3.8). Turning sharing off deletes the
/// outbox at once; reaching the current disclosure wakes the sender so held
/// events go out with their original timestamps.
pub fn set_consent(opt_in: bool, version: u32) {
    let next = if !opt_in {
        Consent::Off
    } else if version >= ANALYTICS_CONSENT_VERSION {
        Consent::On
    } else if version == 0 {
        // After the settings migration, only an unfinished onboarding.
        Consent::Hold
    } else {
        Consent::Unknown
    };
    let previous = std::mem::replace(&mut *lock(&CONSENT), next);
    if previous == next {
        return;
    }
    tracing::info!("analytics: consent {next:?}");
    let Some(core) = CORE.get() else {
        return;
    };
    match next {
        Consent::Off => lock(&core.outbox).purge(),
        Consent::On => core.wake(),
        Consent::Hold | Consent::Unknown => {}
    }
    // Off: the recorder deletes the day file; on: it may owe a summary.
    day::check_day();
}

/// Called by the settings store after every load and save: the consent gate
/// and the model family come from settings.
pub fn apply_settings(settings: &VoiceWaveSettings) {
    context::set_model_family(ModelFamily::of(&settings.active_model));
    set_consent(
        settings.anonymous_usage_opt_in,
        settings.analytics_consent_version,
    );
}

/// Frontend events (plan 3.9). Only the variants matched here may come from
/// the webview; anything else, or a payload that doesn't deserialize, is
/// dropped, so the frontend can't invent events or values. Mirrored by
/// `ClientEvent` in src/types/voicewave.ts.
#[cfg(feature = "desktop")]
#[tauri::command]
pub fn track(event: AnalyticsEvent) {
    match event {
        AnalyticsEvent::OnboardingStep { .. }
        | AnalyticsEvent::OnboardingFinished { .. }
        | AnalyticsEvent::AcquisitionAnswered { .. }
        | AnalyticsEvent::UpdateFailed {
            stage: UpdateStage::Download | UpdateStage::Verify | UpdateStage::Install,
            error_kind: Some(_),
            ..
        } => capture(event),
        _ => tracing::warn!("analytics: the webview can't send {event:?}"),
    }
}

impl Core {
    /// Plan 3.3, resolved now.
    fn base_properties(&self, now_ms: u64) -> Map<String, Value> {
        let live = context::live();
        let mut properties = Map::new();
        let mut insert = |key: &str, value: Value| {
            properties.insert(key.to_string(), value);
        };
        insert("app_version", json!(Semver::current()));
        insert("session_id", json!(self.identity.session_id));
        insert("os_major", json!(self.os_major));
        insert("gpu_vendor", json!(live.gpu_vendor));
        insert("speech_path", json!(live.speech_path));
        insert("model_family", json!(live.model_family));
        insert("install_source", json!(self.identity.install_source));
        insert(
            "install_age",
            json!(now_ms.saturating_sub(self.identity.first_seen_utc_ms) / DAY_MS),
        );
        insert("hypervisor", json!(self.hypervisor));
        insert("$lib", json!("voicewave"));
        insert("$geoip_disable", json!(true));
        if cfg!(debug_assertions) {
            insert("dev_build", json!(true));
        }
        properties
    }

    /// `$set`: the install's current setup, copied from the base properties.
    fn person_properties(&self, base: &Map<String, Value>) -> Value {
        const KEYS: [&str; 7] = [
            "app_version",
            "os_major",
            "gpu_vendor",
            "speech_path",
            "model_family",
            "install_source",
            "hypervisor",
        ];
        Value::Object(
            KEYS.iter()
                .filter_map(|key| Some((key.to_string(), base.get(*key)?.clone())))
                .collect(),
        )
    }

    fn wake(&self) {
        *lock(&self.woken) = true;
        self.wake_signal.notify_one();
    }

    /// Waits up to `timeout`; true when woken by a new event or consent.
    fn wait(&self, timeout: Duration) -> bool {
        let woken = lock(&self.woken);
        let (mut woken, _) = self
            .wake_signal
            .wait_timeout_while(woken, timeout, |woken| !*woken)
            .unwrap_or_else(PoisonError::into_inner);
        std::mem::take(&mut *woken)
    }
}

fn run_sender(core: &'static Core) {
    loop {
        if core.wait(IDLE_FLUSH) {
            std::thread::sleep(DEBOUNCE);
        }
        // A day that ended while the app sat in the tray gets its summary
        // queued here, without waiting for the next dictation.
        if lock(&CONSENT).queues() {
            day::check_day();
        }
        flush(core);
    }
}

/// Sends the outbox oldest first, `MAX_BATCH` per POST, until it's empty or a
/// send fails. A failed batch stays queued for the next pass.
fn flush(core: &Core) {
    loop {
        // The second consent check: sharing may have changed since queueing.
        if *lock(&CONSENT) != Consent::On {
            return;
        }
        let batch = lock(&core.outbox).oldest(now_ms(), MAX_BATCH);
        if batch.is_empty() {
            return;
        }
        let uuids: Vec<String> = batch.iter().map(|entry| entry.uuid.clone()).collect();
        let agent = core.agent.get_or_init(|| {
            crate::net::agent_builder()
                .timeout_connect(Duration::from_secs(10))
                .timeout_read(Duration::from_secs(10))
                .build()
        });
        match posthog::send(agent, &core.api_key, &core.identity.install_id, &batch) {
            SendOutcome::Delivered(status) => {
                tracing::info!("analytics: sent {} events (HTTP {status})", batch.len());
                lock(&core.outbox).remove(&uuids);
            }
            SendOutcome::Rejected(status) => {
                if !REJECTION_LOGGED.swap(true, Ordering::Relaxed) {
                    tracing::warn!(
                        "analytics: PostHog rejected {} events (HTTP {status}); dropped",
                        batch.len()
                    );
                }
                lock(&core.outbox).remove(&uuids);
            }
            SendOutcome::Retry(reason) => {
                tracing::debug!("analytics: send failed ({reason}); will retry");
                return;
            }
        }
    }
}
