//! Crash-safe file replacement and corruption recovery shared by every
//! on-disk store.
//!
//! A plain `fs::write` truncates the target and then streams the new bytes.
//! Losing power inside that window leaves NTFS with the new file *size*
//! committed to the metadata journal but no data blocks flushed, so the next
//! read returns a run of NUL bytes. A store that treats that as a parse error
//! and propagates it out of its constructor bricks the whole app, which is
//! exactly how a 299-byte zero-filled `billing.json` stopped VoiceWave from
//! launching.
//!
//! Two halves fix that:
//!   * [`atomic_write`] never truncates the primary — it writes a `.tmp`
//!     sibling, **fsyncs it**, keeps the previous contents as `.bak`, and only
//!     then renames the temp into place.
//!   * [`load_with_recovery`] finishes an interrupted replace, treats
//!     unusable-empty content as "no file", falls back to the `.bak` sibling,
//!     and quarantines anything it still cannot decode so the caller can boot
//!     on defaults instead of returning `Err`.

use std::{
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

/// Sibling path built by appending `suffix` to the full file name, so
/// `settings.json` yields `settings.json.tmp` rather than `settings.tmp`.
pub fn sibling_with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

/// True when `contents` carries no payload a parser could use.
///
/// This is deliberately wider than `str::trim().is_empty()`: Rust does not
/// classify `\0` as whitespace, so a zero-filled file sails past a `trim`
/// guard and straight into `serde_json::from_str`. NUL runs are the signature
/// of an interrupted write, and a UTF-8 BOM on its own is equally useless.
pub fn is_unusable_empty(contents: &str) -> bool {
    contents
        .chars()
        .all(|value| value == '\0' || value == '\u{feff}' || value.is_whitespace())
}

/// Replaces `path` with `contents` without ever exposing a truncated primary.
///
/// The temp file is flushed with `sync_all` before the rename: `rename` is
/// atomic for *metadata* only, so promoting an unflushed temp can still
/// publish a zero-filled file after a power loss.
pub fn atomic_write(path: &Path, contents: &[u8]) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} has no parent directory", path.display()),
        )
    })?;
    if !parent.as_os_str().is_empty() {
        fs::create_dir_all(parent)?;
    }

    let temporary = sibling_with_suffix(path, ".tmp");
    let backup = sibling_with_suffix(path, ".bak");
    if temporary.exists() {
        fs::remove_file(&temporary)?;
    }
    {
        let mut file = File::create(&temporary)?;
        file.write_all(contents)?;
        file.sync_all()?;
    }

    let had_existing = path.exists();
    if had_existing {
        if backup.exists() {
            fs::remove_file(&backup)?;
        }
        fs::rename(path, &backup)?;
    }
    if let Err(error) = fs::rename(&temporary, path) {
        if had_existing {
            let _ = fs::rename(&backup, path);
        }
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    // A parent-directory fsync would also make the rename entry itself
    // durable, but Windows refuses `File::open` on a directory handle opened
    // this way. The data is already flushed, which is the corruption we hit;
    // an unflushed *directory entry* only costs the newest write, and
    // `recover_interrupted_replace` restores the `.bak` for that case.
    if had_existing {
        // The new primary is already durable. Backup cleanup is best-effort:
        // reporting failure here would roll memory back while disk has the new
        // state. Startup recovery removes a leftover backup safely.
        let _ = fs::remove_file(backup);
    }
    Ok(())
}

/// Finishes a replace that was interrupted between the two renames: restores
/// the `.bak` when the primary vanished, and drops any stale `.tmp`.
pub fn recover_interrupted_replace(path: &Path) -> io::Result<()> {
    let backup = sibling_with_suffix(path, ".bak");
    let temporary = sibling_with_suffix(path, ".tmp");
    if !path.exists() && backup.exists() {
        fs::rename(&backup, path)?;
    }
    if temporary.exists() {
        fs::remove_file(temporary)?;
    }
    Ok(())
}

/// Reads `path` after finishing any interrupted replace. Returns `None` when
/// the file is absent *or* holds nothing usable (see [`is_unusable_empty`]),
/// so callers can treat both as "start from defaults".
pub fn read_to_string_recovering(path: &Path) -> io::Result<Option<String>> {
    recover_interrupted_replace(path)?;
    read_usable(path)
}

/// Moves a file that could not be decoded aside as `<name>.corrupt-<millis>`
/// so it stays available for support without blocking the next write. Returns
/// `Ok(None)` when there was nothing to move.
pub fn quarantine_corrupt(path: &Path) -> io::Result<Option<PathBuf>> {
    if !path.exists() {
        return Ok(None);
    }
    let target = sibling_with_suffix(path, &format!(".corrupt-{}", now_utc_ms()));
    fs::rename(path, &target)?;
    Ok(Some(target))
}

/// What [`load_with_recovery`] found on disk.
pub enum StoreLoad<T> {
    /// Nothing usable was stored yet; keep the caller's defaults.
    Missing,
    /// Decoded straight from the primary file.
    Loaded(T),
    /// The primary was unusable but its `.bak` sibling decoded. The backup has
    /// been promoted back to the primary path.
    Recovered(T),
    /// Neither copy decoded. The primary (when present) has been quarantined
    /// beside itself and logged; the caller must fall back to defaults.
    Reset,
}

/// Loads a store the crash-safe way: finish any interrupted replace, decode
/// the primary, fall back to the `.bak`, and quarantine the rest.
///
/// `decode` is called with the file's text; it may be invoked twice (primary
/// then backup). This never returns an error — an unreadable store must not be
/// able to stop the app from launching.
pub fn load_with_recovery<T, E: std::fmt::Display>(
    path: &Path,
    label: &str,
    mut decode: impl FnMut(&str) -> Result<T, E>,
) -> StoreLoad<T> {
    if let Err(error) = recover_interrupted_replace(path) {
        eprintln!("voicewave: could not finish an interrupted {label} write: {error}");
    }
    let backup = sibling_with_suffix(path, ".bak");

    match read_usable(path) {
        Ok(Some(raw)) => match decode(&raw) {
            Ok(value) => {
                // A stale backup would resurrect older data on the next
                // interrupted write, so drop it once the primary is good.
                if backup.exists() {
                    let _ = fs::remove_file(&backup);
                }
                return StoreLoad::Loaded(value);
            }
            Err(error) => {
                eprintln!("voicewave: {label} store did not decode: {error}");
            }
        },
        Ok(None) => {
            if !backup.exists() {
                return StoreLoad::Missing;
            }
            eprintln!("voicewave: {label} store is empty or zero-filled; trying its backup");
        }
        Err(error) => {
            eprintln!("voicewave: failed to read the {label} store: {error}");
        }
    }

    if let Ok(Some(raw)) = read_usable(&backup) {
        if let Ok(value) = decode(&raw) {
            let _ = fs::remove_file(path);
            match fs::rename(&backup, path) {
                Ok(()) => eprintln!("voicewave: restored the {label} store from its backup"),
                Err(error) => eprintln!(
                    "voicewave: recovered {label} in memory but could not promote its backup: {error}"
                ),
            }
            return StoreLoad::Recovered(value);
        }
    }

    let quarantined = match quarantine_corrupt(path) {
        Ok(target) => target,
        Err(error) => {
            eprintln!("voicewave: could not quarantine the corrupt {label} store: {error}");
            None
        }
    };
    let _ = fs::remove_file(&backup);
    note_store_reset(label);
    match quarantined {
        Some(target) => eprintln!(
            "voicewave: {label} was reset to defaults; the unreadable file is kept at '{}'",
            target.display()
        ),
        None => eprintln!("voicewave: {label} was reset to defaults"),
    }
    StoreLoad::Reset
}

fn read_usable(path: &Path) -> io::Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }
    // Read bytes, not `read_to_string`: a partially written file can hold
    // invalid UTF-8, and that must land in the recovery path rather than
    // surface as an I/O error the caller propagates.
    let bytes = fs::read(path)?;
    let raw = String::from_utf8_lossy(&bytes).into_owned();
    if is_unusable_empty(&raw) {
        return Ok(None);
    }
    Ok(Some(raw))
}

/// Stores that were reset during this process, in first-seen order. Recorded
/// here rather than reported inline because the stores load long before any
/// window exists to tell the user about it.
fn store_resets() -> &'static Mutex<Vec<String>> {
    static RESETS: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    RESETS.get_or_init(|| Mutex::new(Vec::new()))
}

/// Records that `label` fell back to defaults after unreadable data.
pub fn note_store_reset(label: &str) {
    if let Ok(mut resets) = store_resets().lock() {
        if !resets.iter().any(|existing| existing == label) {
            resets.push(label.to_string());
        }
    }
}

/// Drains the recorded resets so the caller can surface a single notice.
pub fn take_store_resets() -> Vec<String> {
    match store_resets().lock() {
        Ok(mut resets) => std::mem::take(&mut *resets),
        Err(_) => Vec::new(),
    }
}

fn now_utc_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("voicewave-atomic-{name}-{}", now_utc_ms()));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn quarantined_siblings(dir: &Path) -> usize {
        fs::read_dir(dir)
            .expect("read temp dir")
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().contains(".corrupt-"))
            .count()
    }

    #[test]
    fn atomic_write_replaces_and_leaves_no_siblings() {
        let dir = temp_dir("replace");
        let path = dir.join("store.json");
        atomic_write(&path, b"{\"a\":1}").expect("first write");
        assert_eq!(fs::read_to_string(&path).expect("read"), "{\"a\":1}");

        atomic_write(&path, b"{\"a\":2}").expect("second write");
        assert_eq!(fs::read_to_string(&path).expect("read"), "{\"a\":2}");
        assert!(!sibling_with_suffix(&path, ".tmp").exists());
        assert!(!sibling_with_suffix(&path, ".bak").exists());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn recover_removes_stale_temp_file() {
        let dir = temp_dir("stale-temp");
        let path = dir.join("store.json");
        fs::write(&path, "{}").expect("seed primary");
        let temporary = sibling_with_suffix(&path, ".tmp");
        fs::write(&temporary, "half").expect("seed temp");

        recover_interrupted_replace(&path).expect("recover");
        assert!(!temporary.exists());
        assert_eq!(fs::read_to_string(&path).expect("read"), "{}");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn recover_restores_backup_when_primary_is_missing() {
        let dir = temp_dir("restore-backup");
        let path = dir.join("store.json");
        let backup = sibling_with_suffix(&path, ".bak");
        fs::write(&backup, "{\"kept\":true}").expect("seed backup");

        recover_interrupted_replace(&path).expect("recover");
        assert!(!backup.exists());
        assert_eq!(fs::read_to_string(&path).expect("read"), "{\"kept\":true}");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn all_nul_content_counts_as_unusable_empty() {
        assert!(is_unusable_empty(""));
        assert!(is_unusable_empty(" \n\t "));
        assert!(is_unusable_empty("\u{feff}   "));
        assert!(is_unusable_empty(&"\0".repeat(299)));
        assert!(!is_unusable_empty("{}"));
        assert!(!is_unusable_empty("\0{}\0"));
    }

    #[test]
    fn read_to_string_recovering_reports_zero_filled_file_as_absent() {
        let dir = temp_dir("zero-filled");
        let path = dir.join("billing.json");
        fs::write(&path, vec![0_u8; 299]).expect("seed zero-filled file");

        let raw = read_to_string_recovering(&path).expect("read");
        assert!(raw.is_none());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn load_with_recovery_falls_back_to_backup_then_quarantines() {
        let dir = temp_dir("fallback");
        let path = dir.join("store.json");
        let backup = sibling_with_suffix(&path, ".bak");
        fs::write(&path, vec![0_u8; 64]).expect("seed corrupt primary");
        fs::write(&backup, "good").expect("seed backup");

        let decode = |raw: &str| -> Result<String, String> {
            if raw == "good" {
                Ok(raw.to_string())
            } else {
                Err("bad payload".to_string())
            }
        };

        match load_with_recovery(&path, "test-store", decode) {
            StoreLoad::Recovered(value) => assert_eq!(value, "good"),
            _ => panic!("expected recovery from the backup sibling"),
        }
        assert!(!backup.exists());
        assert_eq!(fs::read_to_string(&path).expect("read"), "good");

        // Now break both copies: the primary must be quarantined, not fatal.
        fs::write(&path, "garbage").expect("seed garbage");
        assert!(matches!(
            load_with_recovery(&path, "test-store", decode),
            StoreLoad::Reset
        ));
        assert!(!path.exists(), "the corrupt primary should be moved aside");
        assert!(
            quarantined_siblings(&dir) == 1,
            "the corrupt file should be kept for support"
        );
        assert!(take_store_resets()
            .iter()
            .any(|label| label == "test-store"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn load_with_recovery_reports_missing_when_nothing_is_stored() {
        let dir = temp_dir("missing");
        let path = dir.join("store.json");
        let outcome = load_with_recovery(&path, "test-missing", |raw: &str| {
            serde_json::from_str::<serde_json::Value>(raw)
        });
        assert!(matches!(outcome, StoreLoad::Missing));

        let _ = fs::remove_dir_all(dir);
    }
}
