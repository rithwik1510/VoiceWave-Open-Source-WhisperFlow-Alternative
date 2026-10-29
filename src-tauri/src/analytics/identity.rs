//! Who is sending (plan 3.2): a random install id, a random id per launch,
//! and when this install was first seen. Kept in `HKCU\Software\VoiceWave\
//! LocalCore`, next to the installer's `InstallSource`, so it survives
//! updates and reinstalls; only an uninstall with "Delete app data" removes
//! the key. No account, hardware or machine ids, ever.

use serde::Serialize;
use std::sync::OnceLock;
use uuid::Uuid;

const KEY_PATH: &str = r"Software\VoiceWave\LocalCore";
const INSTALL_ID: &str = "InstallId";
const FIRST_SEEN_UTC_MS: &str = "FirstSeenUtcMs";
/// 1 when backfilled for an install that predates 0.7.7.
const FIRST_SEEN_ESTIMATED: &str = "FirstSeenEstimated";
/// The app version that first recorded these values.
const FIRST_SEEN_VERSION: &str = "FirstSeenVersion";
/// Written by the NSIS installer (plan section 6); only read here.
const INSTALL_SOURCE: &str = "InstallSource";

static SETTINGS_EXISTED_AT_LAUNCH: OnceLock<bool> = OnceLock::new();

/// Records whether `settings.json` existed before this launch could write it,
/// which is what tells a pre-0.7.7 install from a new one. Must run at the
/// very start of setup; later calls keep the first answer.
pub fn note_settings_existed_at_launch() -> bool {
    *SETTINGS_EXISTED_AT_LAUNCH.get_or_init(|| {
        crate::settings::SettingsStore::new().is_ok_and(|store| store.path().exists())
    })
}

/// How VoiceWave was installed, as the installer recorded it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallSource {
    Winget,
    Direct,
    Unknown,
}

impl InstallSource {
    fn parse(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            Some("winget") => Self::Winget,
            Some("direct") => Self::Direct,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Identity {
    /// PostHog's `distinct_id`.
    pub install_id: String,
    pub session_id: String,
    pub first_seen_utc_ms: u64,
    pub first_seen_estimated: bool,
    pub first_seen_version: String,
    pub install_source: InstallSource,
    /// This launch created the first-seen facts of a new (not pre-0.7.7)
    /// install: the install's first run.
    pub first_launch: bool,
}

/// Reads the stored identity, creating the install id and first-seen facts on
/// the first launch of 0.7.7 or later. `None` when the registry key can't be
/// opened or the install id can't be stored: without a stable id every launch
/// would look like a new person, so analytics stays off for this run instead.
#[cfg(windows)]
pub fn load_or_create(now_ms: u64) -> Option<Identity> {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};

    let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey(KEY_PATH)
        .map_err(|err| tracing::warn!("analytics: registry key unavailable ({err})"))
        .ok()?;
    let read_string = |name: &str| key.get_value::<String, _>(name).ok();

    let install_id = match read_string(INSTALL_ID).and_then(|id| Uuid::try_parse(id.trim()).ok())
    {
        Some(id) => id.to_string(),
        None => {
            let id = Uuid::new_v4().to_string();
            key.set_value(INSTALL_ID, &id)
                .map_err(|err| tracing::warn!("analytics: couldn't store the install id ({err})"))
                .ok()?;
            id
        }
    };

    let (first_seen_utc_ms, first_seen_estimated, first_seen_version, first_launch) =
        match key.get_value::<u64, _>(FIRST_SEEN_UTC_MS) {
            Ok(ms) => (
                ms,
                key.get_value::<u32, _>(FIRST_SEEN_ESTIMATED).ok() == Some(1),
                read_string(FIRST_SEEN_VERSION).unwrap_or_default(),
                false,
            ),
            Err(_) => {
                let estimated = note_settings_existed_at_launch();
                // An install that predates 0.7.7 was first seen when its
                // settings file was created.
                let ms = estimated
                    .then(settings_created_ms)
                    .flatten()
                    .unwrap_or(now_ms);
                let version = env!("CARGO_PKG_VERSION").to_string();
                let stored = key
                    .set_value(FIRST_SEEN_UTC_MS, &ms)
                    .and_then(|()| key.set_value(FIRST_SEEN_ESTIMATED, &u32::from(estimated)))
                    .and_then(|()| key.set_value(FIRST_SEEN_VERSION, &version));
                if let Err(err) = stored {
                    tracing::warn!("analytics: couldn't store first-seen facts ({err})");
                }
                (ms, estimated, version, !estimated)
            }
        };

    Some(Identity {
        install_id,
        session_id: Uuid::new_v4().to_string(),
        first_seen_utc_ms,
        first_seen_estimated,
        first_seen_version,
        install_source: InstallSource::parse(read_string(INSTALL_SOURCE).as_deref()),
        first_launch,
    })
}

#[cfg(not(windows))]
pub fn load_or_create(_now_ms: u64) -> Option<Identity> {
    None
}

fn settings_created_ms() -> Option<u64> {
    let store = crate::settings::SettingsStore::new().ok()?;
    let created = std::fs::metadata(store.path()).ok()?.created().ok()?;
    created
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|age| age.as_millis() as u64)
}
