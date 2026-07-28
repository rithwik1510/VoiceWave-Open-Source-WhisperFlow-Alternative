use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::path::Path;

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

/// Loads the key at `path`, regenerating it when the file is missing or
/// unreadable.
///
/// A key file can be zero-filled by the same interrupted-write mechanism that
/// corrupts stores. Refusing to launch over it is the worst possible outcome:
/// a fresh key means the encrypted store no longer decrypts, and the store's
/// own recovery path then quarantines it and starts from defaults. Losing a
/// local cache beats an app that cannot start.
pub(crate) fn load_or_create_key(
    path: &Path,
    label: &str,
) -> Result<[u8; KEY_BYTES], SecureStoreError> {
    let stored = crate::atomic_file::read_to_string_recovering(path).unwrap_or_else(|error| {
        eprintln!("voicewave: failed to read the {label} key, regenerating: {error}");
        None
    });

    if let Some(encoded) = stored {
        match decode_key(&encoded, label) {
            Ok(key) => return Ok(key),
            Err(error) => {
                eprintln!("voicewave: {label} key is unusable, regenerating: {error}");
                if let Err(error) = crate::atomic_file::quarantine_corrupt(path) {
                    eprintln!("voicewave: could not quarantine the {label} key: {error}");
                }
                crate::atomic_file::note_store_reset(label);
            }
        }
    }

    let mut key = [0_u8; KEY_BYTES];
    OsRng.fill_bytes(&mut key);
    let encoded = base64::engine::general_purpose::STANDARD.encode(key);
    crate::atomic_file::atomic_write(path, encoded.as_bytes()).map_err(|source| {
        SecureStoreError::Write {
            label: label.to_string(),
            source,
        }
    })?;
    Ok(key)
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
