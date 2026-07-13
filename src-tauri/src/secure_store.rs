use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

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
    #[error("failed to read {label}: {source}")]
    Read {
        label: String,
        #[source]
        source: std::io::Error,
    },
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

pub(crate) fn load_or_create_key(
    path: &Path,
    label: &str,
) -> Result<[u8; KEY_BYTES], SecureStoreError> {
    if path.exists() {
        let encoded = fs::read_to_string(path).map_err(|source| SecureStoreError::Read {
            label: label.to_string(),
            source,
        })?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded.trim())
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
        return Ok(key);
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| SecureStoreError::Write {
            label: label.to_string(),
            source,
        })?;
    }
    let mut key = [0_u8; KEY_BYTES];
    OsRng.fill_bytes(&mut key);
    let encoded = base64::engine::general_purpose::STANDARD.encode(key);
    fs::write(path, encoded).map_err(|source| SecureStoreError::Write {
        label: label.to_string(),
        source,
    })?;
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
