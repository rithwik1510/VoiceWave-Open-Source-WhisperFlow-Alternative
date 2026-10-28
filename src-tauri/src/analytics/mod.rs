//! Usage analytics on PostHog (plan 024), sent from Rust.
//!
//! `capture` resolves an `AnalyticsEvent` plus the base properties into an
//! entry in the persisted outbox; one background thread sends due entries to
//! PostHog with their real timestamps. Nothing happens without a project key
//! (section 3.5), and the consent gate (3.8) is checked when an event is
//! queued and again before each send. Privacy rules: section 2 of the plan.
//! The older TelemetryDeck path (`crate::telemetry`, src/lib/telemetry.ts)
//! is separate and unchanged.

mod context;
mod events;
mod identity;
mod outbox;
mod posthog;

pub use context::note_speech_engine;
pub use events::{AnalyticsEvent, ClientEvent, LaunchMode};

use crate::settings::{
    VoiceWaveSettings, ANALYTICS_CONSENT_PROMPT_MAX_SHOWS, ANALYTICS_CONSENT_VERSION,
};
use context::{Hypervisor, ModelFamily, OsMajor};
use identity::Identity;
use outbox::{Outbox, OutboxEntry};
use posthog::SendOutcome;
use rand::Rng;
use serde_json::{json, Map, Value};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Condvar, Mutex, MutexGuard, OnceLock, PoisonError, TryLockError,
    },
    time::{Duration, Instant},
};

const OUTBOX_FILE: &str = "analytics-outbox.json";
/// Events per `/batch/` POST.
const MAX_BATCH: usize = 50;
/// How often the sender looks at the outbox without being woken.
const IDLE_FLUSH: Duration = Duration::from_secs(60);
/// After a wake, so a burst of events goes out in one POST.
const DEBOUNCE: Duration = Duration::from_secs(2);
/// Total time the quit-time flush may take.
const EXIT_BUDGET: Duration = Duration::from_secs(2);
const MAX_BACKOFF_MS: u64 = 5 * 60 * 1000;
const DAY_MS: u64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Consent {
    /// Settings not loaded yet: nothing is queued or sent.
    Unknown,
    /// Sharing off, or the v2 prompt went unanswered too often: nothing is
    /// queued and the outbox is deleted.
    Off,
    /// On under an older disclosure (or onboarding not finished): queue, but
    /// send only once the user has seen the current one.
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
    /// Held for a whole flush, so the sender thread and the quit-time flush
    /// never send the same entries at once.
    sending: Mutex<()>,
    woken: Mutex<bool>,
    wake_signal: Condvar,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn now() -> (u64, String) {
    let now = chrono::Utc::now();
    (
        u64::try_from(now.timestamp_millis()).unwrap_or_default(),
        now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
    )
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
    let (now_ms, _) = now();
    let Some(identity) = identity::load_or_create(now_ms) else {
        return;
    };
    start(api_key, identity, data_dir.join(OUTBOX_FILE));
}

fn start(api_key: String, identity: Identity, outbox_path: PathBuf) {
    let (now_ms, _) = now();
    let core = Core {
        api_key,
        identity,
        os_major: OsMajor::current(),
        hypervisor: Hypervisor::detect(),
        outbox: Mutex::new(Outbox::load(outbox_path, now_ms)),
        agent: OnceLock::new(),
        sending: Mutex::new(()),
        woken: Mutex::new(false),
        wake_signal: Condvar::new(),
    };
    if CORE.set(core).is_err() {
        return;
    }
    let Some(core) = CORE.get() else {
        return;
    };
    {
        let consent = lock(&CONSENT);
        if *consent == Consent::Off {
            lock(&core.outbox).purge();
        }
    }
    if let Err(err) = std::thread::Builder::new()
        .name("analytics-sender".to_string())
        .spawn(move || run_sender(core))
    {
        tracing::warn!("analytics: couldn't start the sender: {err}");
    }
}

/// Queues `event` when sharing allows it. Never blocks on the network.
pub fn capture(event: AnalyticsEvent) {
    enqueue(event);
}

/// Returns whether the event was queued. Holds the consent lock until the
/// entry is stored, so a switch-off can't purge in between and leave it
/// behind (lock order everywhere: consent, then outbox).
fn enqueue(event: AnalyticsEvent) -> bool {
    let Some(core) = CORE.get() else {
        return false;
    };
    let consent = lock(&CONSENT);
    if !consent.queues() {
        return false;
    }
    let Ok(Value::Object(mut properties)) = serde_json::to_value(event) else {
        return false;
    };
    let Some(Value::String(name)) = properties.remove("event") else {
        return false;
    };
    let (now_ms, timestamp) = now();
    let base = core.base_properties(now_ms);
    if event.sets_person() {
        properties.insert("$set".to_string(), core.person_properties(&base));
        properties.insert(
            "$set_once".to_string(),
            json!({
                "first_version": semver_or_other(&core.identity.first_seen_version),
                "first_install_source": core.identity.install_source,
            }),
        );
    }
    if let AnalyticsEvent::AcquisitionAnswered { answer } = event {
        properties.insert("$set_once".to_string(), json!({ "acquisition": answer }));
    }
    properties.extend(base);
    lock(&core.outbox).push(
        OutboxEntry {
            uuid: identity::uuid_v4(),
            event: name,
            timestamp,
            properties,
            attempts: 0,
            next_at_ms: 0,
        },
        now_ms,
    );
    drop(consent);
    core.wake();
    true
}

/// Queues the startup events: `app_installed` once per install (including
/// an install first launched with sharing off, once it's turned on), then
/// `app_launched`. Call after settings have loaded.
pub fn capture_launch(launch_mode: LaunchMode) {
    let Some(core) = CORE.get() else {
        return;
    };
    if !core.identity.app_installed_queued
        && enqueue(AnalyticsEvent::AppInstalled {
            estimated: core.identity.first_seen_estimated,
        })
    {
        identity::mark_app_installed_queued();
    }
    enqueue(AnalyticsEvent::AppLaunched { launch_mode });
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
pub fn set_consent(opt_in: bool, version: u32, prompt_exhausted: bool) {
    let next = if !opt_in || (version < ANALYTICS_CONSENT_VERSION && prompt_exhausted) {
        Consent::Off
    } else if version >= ANALYTICS_CONSENT_VERSION {
        Consent::On
    } else {
        Consent::Hold
    };
    let mut consent = lock(&CONSENT);
    let previous = std::mem::replace(&mut *consent, next);
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
}

/// Called by the settings store after every load and save: the consent gate
/// and the model family come from settings.
pub fn apply_settings(settings: &VoiceWaveSettings) {
    context::set_model_family(ModelFamily::of(&settings.active_model));
    set_consent(
        settings.anonymous_usage_opt_in,
        settings.analytics_consent_version,
        settings.analytics_consent_prompt_shows >= ANALYTICS_CONSENT_PROMPT_MAX_SHOWS,
    );
}

/// One best-effort flush when the app quits, within `EXIT_BUDGET`. Whatever
/// is left waits in the outbox for the next launch.
pub fn flush_on_exit() {
    if let Some(core) = CORE.get() {
        flush(core, Some(Instant::now() + EXIT_BUDGET));
    }
}

/// Frontend events (plan 3.9). A payload that isn't a known `ClientEvent`
/// fails to deserialize, so nothing is queued.
#[cfg(feature = "desktop")]
#[tauri::command]
pub fn track(event: ClientEvent) {
    capture(event.into());
}

/// The app version, or `other` for anything that isn't plain semver (the
/// only free-form string the plan allows, section 2).
fn semver_or_other(version: &str) -> &str {
    let mut parts = version.split('.');
    let is_semver = parts.clone().count() == 3
        && parts.all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()));
    if is_semver {
        version
    } else {
        "other"
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
        insert("app_version", json!(env!("CARGO_PKG_VERSION")));
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
        flush(core, None);
    }
}

/// When a failed entry is next tried: `min(2 s × 2^attempts, 5 min)` ± 20%.
fn retry_at(now_ms: u64, attempts: u32) -> u64 {
    let backoff = 2_000u64
        .saturating_mul(1u64 << attempts.min(20))
        .min(MAX_BACKOFF_MS);
    let jitter = rand::thread_rng().gen_range(0.8..=1.2);
    now_ms + (backoff as f64 * jitter) as u64
}

/// Sends due entries, oldest first, until none are due or a send fails.
/// With a deadline (quit) it skips when a flush is already running and caps
/// each request at the time left.
fn flush(core: &Core, deadline: Option<Instant>) {
    let _sending = match deadline {
        None => lock(&core.sending),
        Some(_) => match core.sending.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => return,
        },
    };
    loop {
        // The second consent check: sharing may have changed since queueing.
        if *lock(&CONSENT) != Consent::On {
            return;
        }
        let (now_ms, _) = now();
        let batch = lock(&core.outbox).due(now_ms, MAX_BATCH);
        if batch.is_empty() {
            return;
        }
        let timeout = match deadline {
            Some(deadline) => {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return;
                }
                Some(left)
            }
            None => None,
        };
        let uuids: Vec<String> = batch.iter().map(|entry| entry.uuid.clone()).collect();
        let agent = core.agent.get_or_init(|| {
            crate::net::agent_builder()
                .timeout_connect(Duration::from_secs(10))
                .timeout_read(Duration::from_secs(10))
                .build()
        });
        match posthog::send(
            agent,
            &core.api_key,
            &core.identity.install_id,
            &batch,
            timeout,
        ) {
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
                let (now_ms, _) = now();
                lock(&core.outbox).defer(&uuids, |attempts| retry_at(now_ms, attempts));
                return;
            }
        }
    }
}
