//! Who is sending (plan 3.2): a random install id, a random id per launch,
//! and when this install was first seen. Kept in `HKCU\Software\VoiceWave\
//! LocalCore`, next to the installer's `InstallSource`, so it survives
//! updates and reinstalls; only an uninstall with "Delete app data" removes
//! the key. No account, hardware or machine ids, ever.

use rand::RngCore;
use serde::Serialize;
use std::sync::OnceLock;

const INSTALL_ID: &str = "InstallId";
const FIRST_SEEN_UTC_MS: &str = "FirstSeenUtcMs";
/// 1 when backfilled for an install that predates 0.7.7.
const FIRST_SEEN_ESTIMATED: &str = "FirstSeenEstimated";
/// The app version that first recorded these values.
const FIRST_SEEN_VERSION: &str = "FirstSeenVersion";
/// Written by the NSIS installer (plan section 6); only read here.
const INSTALL_SOURCE: &str = "InstallSource";
/// 1 once `app_installed` has been queued, so an install first launched with
/// sharing off still reports it once sharing is turned on.
const APP_INSTALLED_QUEUED: &str = "AppInstalledQueued";
/// 1 once `first_dictation` has been queued, so it's sent once per install.
const FIRST_DICTATION_QUEUED: &str = "FirstDictationQueued";

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
    Scoop,
    Direct,
    Unknown,
}

impl InstallSource {
    fn parse(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            Some("winget") => Self::Winget,
            Some("scoop") => Self::Scoop,
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
    pub app_installed_queued: bool,
    pub first_dictation_queued: bool,
    /// This launch created the first-seen facts of a new (not pre-0.7.7)
    /// install: the install's first run.
    pub first_launch: bool,
}

/// A random UUID v4 string.
pub fn uuid_v4() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

fn is_uuid(value: &str) -> bool {
    value.len() == 36
        && value.char_indices().all(|(index, c)| match index {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

/// Reads the stored identity, creating the install id and first-seen facts on
/// the first launch of 0.7.7 or later. `None` when the registry key can't be
/// opened or the install id can't be stored: without a stable id every launch
/// would look like a new person, so analytics stays off for this run instead.
#[cfg(windows)]
pub fn load_or_create(now_ms: u64) -> Option<Identity> {
    let key = registry::Key::open_or_create()
        .map_err(|code| tracing::warn!("analytics: registry key unavailable (error {code})"))
        .ok()?;

    let install_id = match key.read_string(INSTALL_ID).filter(|id| is_uuid(id)) {
        Some(id) => id,
        None => {
            let id = uuid_v4();
            key.write_string(INSTALL_ID, &id)
                .map_err(|code| {
                    tracing::warn!("analytics: couldn't store the install id (error {code})")
                })
                .ok()?;
            id
        }
    };

    let (first_seen_utc_ms, first_seen_estimated, first_seen_version, first_launch) =
        match key.read_u64(FIRST_SEEN_UTC_MS) {
            Some(ms) => (
                ms,
                key.read_u32(FIRST_SEEN_ESTIMATED) == Some(1),
                key.read_string(FIRST_SEEN_VERSION).unwrap_or_default(),
                false,
            ),
            None => {
                let estimated = note_settings_existed_at_launch();
                // An install that predates 0.7.7 was first seen when its
                // settings file was created.
                let ms = estimated
                    .then(settings_created_ms)
                    .flatten()
                    .unwrap_or(now_ms);
                let version = env!("CARGO_PKG_VERSION").to_string();
                let stored = key
                    .write_u64(FIRST_SEEN_UTC_MS, ms)
                    .and_then(|()| key.write_u32(FIRST_SEEN_ESTIMATED, u32::from(estimated)))
                    .and_then(|()| key.write_string(FIRST_SEEN_VERSION, &version));
                if let Err(code) = stored {
                    tracing::warn!("analytics: couldn't store first-seen facts (error {code})");
                }
                (ms, estimated, version, !estimated)
            }
        };

    Some(Identity {
        install_id,
        session_id: uuid_v4(),
        first_seen_utc_ms,
        first_seen_estimated,
        first_seen_version,
        install_source: InstallSource::parse(key.read_string(INSTALL_SOURCE).as_deref()),
        app_installed_queued: key.read_u32(APP_INSTALLED_QUEUED) == Some(1),
        first_dictation_queued: key.read_u32(FIRST_DICTATION_QUEUED) == Some(1),
        first_launch,
    })
}

#[cfg(not(windows))]
pub fn load_or_create(_now_ms: u64) -> Option<Identity> {
    None
}

/// Remembers that `app_installed` is queued, so it's never queued twice.
pub fn mark_app_installed_queued() {
    mark_queued(APP_INSTALLED_QUEUED);
}

/// Remembers that `first_dictation` is queued, so it's never queued twice.
pub fn mark_first_dictation_queued() {
    mark_queued(FIRST_DICTATION_QUEUED);
}

#[cfg(windows)]
fn mark_queued(name: &str) {
    if let Err(code) = registry::Key::open_or_create().and_then(|key| key.write_u32(name, 1)) {
        tracing::warn!("analytics: couldn't record {name} (error {code})");
    }
}

#[cfg(not(windows))]
fn mark_queued(_name: &str) {}

fn settings_created_ms() -> Option<u64> {
    let store = crate::settings::SettingsStore::new().ok()?;
    let created = std::fs::metadata(store.path()).ok()?.created().ok()?;
    created
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|age| age.as_millis() as u64)
}

/// The few registry calls identity needs, on the one key it owns.
#[cfg(windows)]
mod registry {
    use windows_sys::Win32::{
        Foundation::ERROR_SUCCESS,
        System::Registry::{
            RegCloseKey, RegCreateKeyExW, RegGetValueW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
            KEY_QUERY_VALUE, KEY_SET_VALUE, REG_DWORD, REG_OPTION_NON_VOLATILE, REG_QWORD, REG_SZ,
            RRF_RT_REG_DWORD, RRF_RT_REG_QWORD, RRF_RT_REG_SZ,
        },
    };

    const KEY_PATH: &str = r"Software\VoiceWave\LocalCore";

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// An open `HKCU\Software\VoiceWave\LocalCore`, closed on drop. Errors
    /// are Win32 codes, for the log.
    pub struct Key(HKEY);

    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: the handle came from RegCreateKeyExW and is closed once.
            unsafe { RegCloseKey(self.0) };
        }
    }

    impl Key {
        pub fn open_or_create() -> Result<Self, u32> {
            let path = wide(KEY_PATH);
            let mut handle: HKEY = std::ptr::null_mut();
            // SAFETY: `path` is nul-terminated UTF-16 that outlives the call;
            // null class, security attributes and disposition are allowed.
            let status = unsafe {
                RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    path.as_ptr(),
                    0,
                    std::ptr::null(),
                    REG_OPTION_NON_VOLATILE,
                    KEY_QUERY_VALUE | KEY_SET_VALUE,
                    std::ptr::null(),
                    &mut handle,
                    std::ptr::null_mut(),
                )
            };
            if status == ERROR_SUCCESS {
                Ok(Self(handle))
            } else {
                Err(status)
            }
        }

        /// Reads into `buffer` with `flags`; returns the bytes written.
        fn read(&self, name: &str, flags: u32, buffer: &mut [u8]) -> Option<usize> {
            let name = wide(name);
            let mut size = buffer.len() as u32;
            // SAFETY: `name` is nul-terminated; `size` is `buffer`'s byte
            // length and the call writes at most that many bytes.
            let status = unsafe {
                RegGetValueW(
                    self.0,
                    std::ptr::null(),
                    name.as_ptr(),
                    flags,
                    std::ptr::null_mut(),
                    buffer.as_mut_ptr().cast(),
                    &mut size,
                )
            };
            (status == ERROR_SUCCESS).then_some(size as usize)
        }

        /// Values here are short (ids, versions, a source name), so a fixed
        /// buffer is enough; anything longer reads as absent.
        pub fn read_string(&self, name: &str) -> Option<String> {
            let mut buffer = [0u8; 512];
            let size = self.read(name, RRF_RT_REG_SZ, &mut buffer)?;
            let units: Vec<u16> = buffer[..size]
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .take_while(|&unit| unit != 0)
                .collect();
            let value = String::from_utf16_lossy(&units).trim().to_string();
            (!value.is_empty()).then_some(value)
        }

        pub fn read_u32(&self, name: &str) -> Option<u32> {
            let mut buffer = [0u8; 4];
            self.read(name, RRF_RT_REG_DWORD, &mut buffer)?;
            Some(u32::from_le_bytes(buffer))
        }

        pub fn read_u64(&self, name: &str) -> Option<u64> {
            let mut buffer = [0u8; 8];
            self.read(name, RRF_RT_REG_QWORD, &mut buffer)?;
            Some(u64::from_le_bytes(buffer))
        }

        fn write(&self, name: &str, kind: u32, data: &[u8]) -> Result<(), u32> {
            let name = wide(name);
            // SAFETY: `name` is nul-terminated and `data` is valid for its
            // length, which is what is passed.
            let status = unsafe {
                RegSetValueExW(
                    self.0,
                    name.as_ptr(),
                    0,
                    kind,
                    data.as_ptr(),
                    data.len() as u32,
                )
            };
            if status == ERROR_SUCCESS {
                Ok(())
            } else {
                Err(status)
            }
        }

        pub fn write_string(&self, name: &str, value: &str) -> Result<(), u32> {
            let bytes: Vec<u8> = wide(value).into_iter().flat_map(u16::to_le_bytes).collect();
            self.write(name, REG_SZ, &bytes)
        }

        pub fn write_u32(&self, name: &str, value: u32) -> Result<(), u32> {
            self.write(name, REG_DWORD, &value.to_le_bytes())
        }

        pub fn write_u64(&self, name: &str, value: u64) -> Result<(), u32> {
            self.write(name, REG_QWORD, &value.to_le_bytes())
        }
    }
}
