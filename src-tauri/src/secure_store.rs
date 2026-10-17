use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::{path::Path, time::Duration};

const ENVELOPE_VERSION: u8 = 1;
const KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 12;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EncryptedEnvelope {
    pub(crate) version: u8,
    pub(crate) nonce_b64: String,
    pub(crate) ciphertext_b64: String,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum SecureStoreError {
    #[error("failed to write {label}: {source}")]
    Write {
        label: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to serialize {label}: {source}")]
    Serialize {
        label: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("failed to encrypt {label}: {message}")]
    Encrypt { label: String, message: String },
    #[error("failed to decrypt {label}: {message}")]
    Decrypt { label: String, message: String },
    #[error("failed to decode {label} key: {message}")]
    KeyDecode { label: String, message: String },
}

/// Read attempts before an existing key file that will not open is treated as
/// stuck. Antivirus scanners and backup agents hold short sharing locks on
/// Windows, so the first failure is usually transient.
const KEY_READ_ATTEMPTS: u32 = 5;
/// First retry delay; it doubles per attempt (50, 100, 200, 400 ms).
const KEY_READ_BACKOFF_MS: u64 = 50;

/// What was found at a key path.
enum KeyFile {
    /// Nothing there yet: a first run, or the key was deliberately removed.
    Missing,
    /// The raw file contents, not yet validated.
    Present(String),
    /// The file exists but kept failing to read after every retry.
    Unreadable(std::io::Error),
}

/// Loads the key at `path`, creating it when none exists yet.
///
/// Every store encrypted with this key becomes undecryptable the moment the
/// key changes, so an existing key file is never overwritten in place:
///
/// | On disk                               | Outcome                                    |
/// |---------------------------------------|--------------------------------------------|
/// | missing                               | generate and persist a new key             |
/// | valid                                 | use it                                     |
/// | does not decode (e.g. zero-filled)    | set aside as `.corrupt-<ms>`, regenerate   |
/// | unreadable after retries              | set aside as `.corrupt-<ms>`, regenerate   |
/// | either of the above, rename fails too | session-only key, file left untouched      |
///
/// Refusing to launch is the worst possible outcome, so this only errors when
/// a brand-new key cannot be written. After a regenerate or a session-only
/// key the store's own recovery quarantines the data it can no longer
/// decrypt; the old key and that data both stay on disk for support. A
/// session-only key is never written, so the next launch retries the
/// original file.
pub(crate) fn load_or_create_key(
    path: &Path,
    label: &str,
) -> Result<[u8; KEY_BYTES], SecureStoreError> {
    let unusable = match read_key_file(path, label) {
        KeyFile::Missing => None,
        KeyFile::Present(encoded) => match decode_key(&encoded, label) {
            Ok(key) => return Ok(key),
            Err(error) => Some(error.to_string()),
        },
        KeyFile::Unreadable(error) => Some(format!("still unreadable after retries: {error}")),
    };

    if let Some(reason) = unusable {
        crate::atomic_file::note_store_reset(label);
        match crate::atomic_file::quarantine_corrupt(path) {
            Ok(Some(target)) => tracing::warn!(
                "{label} key is unusable ({reason}); kept it at '{}' and regenerating",
                target.display()
            ),
            Ok(None) => tracing::warn!("{label} key vanished while unusable; regenerating"),
            Err(error) => {
                // Writing now would replace the only copy of a key that may
                // still be fine once the lock clears. Run this session on a
                // throwaway key and leave the file for the next launch.
                tracing::error!(
                    "{label} key is unusable ({reason}) and could not be moved aside \
                     ({error}); using a session-only key and leaving the file untouched"
                );
                return Ok(random_key());
            }
        }
    }

    let key = random_key();
    let encoded = base64::engine::general_purpose::STANDARD.encode(key);
    crate::atomic_file::atomic_write(path, encoded.as_bytes()).map_err(|source| {
        SecureStoreError::Write {
            label: label.to_string(),
            source,
        }
    })?;
    Ok(key)
}

/// Reads the key file, retrying anything but `NotFound` with backoff. Only a
/// genuine `NotFound` counts as missing: any other error means the file is
/// (or may be) there and must not be replaced.
fn read_key_file(path: &Path, label: &str) -> KeyFile {
    if let Err(error) = crate::atomic_file::recover_interrupted_replace(path) {
        tracing::warn!("could not finish an interrupted {label} key write: {error}");
    }
    let mut attempt = 1;
    loop {
        match std::fs::read(path) {
            // Lossy: a damaged key must reach `decode_key` and its quarantine
            // path rather than surface as an I/O error.
            Ok(bytes) => return KeyFile::Present(String::from_utf8_lossy(&bytes).into_owned()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return KeyFile::Missing,
            Err(error) if attempt >= KEY_READ_ATTEMPTS => return KeyFile::Unreadable(error),
            Err(error) => {
                tracing::warn!(
                    "could not read the {label} key (attempt {attempt}), retrying: {error}"
                );
                std::thread::sleep(Duration::from_millis(KEY_READ_BACKOFF_MS << (attempt - 1)));
                attempt += 1;
            }
        }
    }
}

fn random_key() -> [u8; KEY_BYTES] {
    let mut key = [0_u8; KEY_BYTES];
    OsRng.fill_bytes(&mut key);
    key
}

fn decode_key(encoded: &str, label: &str) -> Result<[u8; KEY_BYTES], SecureStoreError> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim().trim_matches('\0').trim())
        .map_err(|error| SecureStoreError::KeyDecode {
            label: label.to_string(),
            message: error.to_string(),
        })?;
    if bytes.len() != KEY_BYTES {
        return Err(SecureStoreError::KeyDecode {
            label: label.to_string(),
            message: format!("{label}.key must decode to {KEY_BYTES} bytes"),
        });
    }

    let mut key = [0_u8; KEY_BYTES];
    key.copy_from_slice(&bytes);
    Ok(key)
}

pub(crate) fn encrypt_json<T: Serialize>(
    value: &T,
    key: &[u8; KEY_BYTES],
    label: &str,
) -> Result<EncryptedEnvelope, SecureStoreError> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|error| SecureStoreError::Encrypt {
        label: label.to_string(),
        message: error.to_string(),
    })?;
    let plaintext = serde_json::to_vec(value).map_err(|source| SecureStoreError::Serialize {
        label: label.to_string(),
        source,
    })?;
    let mut nonce_bytes = [0_u8; NONCE_BYTES];
    OsRng.fill_bytes(&mut nonce_bytes);
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), plaintext.as_ref())
        .map_err(|error| SecureStoreError::Encrypt {
            label: label.to_string(),
            message: error.to_string(),
        })?;

    Ok(EncryptedEnvelope {
        version: ENVELOPE_VERSION,
        nonce_b64: base64::engine::general_purpose::STANDARD.encode(nonce_bytes),
        ciphertext_b64: base64::engine::general_purpose::STANDARD.encode(ciphertext),
    })
}

pub(crate) fn decrypt_bytes(
    encrypted: &EncryptedEnvelope,
    key: &[u8; KEY_BYTES],
    label: &str,
) -> Result<Vec<u8>, SecureStoreError> {
    if encrypted.version != ENVELOPE_VERSION {
        return Err(SecureStoreError::Decrypt {
            label: label.to_string(),
            message: format!(
                "unsupported {label} encryption version {}",
                encrypted.version
            ),
        });
    }

    let nonce_bytes = base64::engine::general_purpose::STANDARD
        .decode(encrypted.nonce_b64.as_bytes())
        .map_err(|error| SecureStoreError::Decrypt {
            label: label.to_string(),
            message: error.to_string(),
        })?;
    let ciphertext = base64::engine::general_purpose::STANDARD
        .decode(encrypted.ciphertext_b64.as_bytes())
        .map_err(|error| SecureStoreError::Decrypt {
            label: label.to_string(),
            message: error.to_string(),
        })?;
    if nonce_bytes.len() != NONCE_BYTES {
        return Err(SecureStoreError::Decrypt {
            label: label.to_string(),
            message: format!("{label} nonce must be {NONCE_BYTES} bytes"),
        });
    }

    let cipher = Aes256Gcm::new_from_slice(key).map_err(|error| SecureStoreError::Decrypt {
        label: label.to_string(),
        message: error.to_string(),
    })?;
    cipher
        .decrypt(Nonce::from_slice(&nonce_bytes), ciphertext.as_ref())
        .map_err(|error| SecureStoreError::Decrypt {
            label: label.to_string(),
            message: error.to_string(),
        })
}
