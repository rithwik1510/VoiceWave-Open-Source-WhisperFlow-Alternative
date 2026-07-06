use crate::insertion::{InsertResult, InsertionMethod};
use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use base64::Engine;
use directories::ProjectDirs;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

/// Count cap on stored records, on top of the time-based RetentionPolicy below.
const MAX_HISTORY_RECORDS: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RetentionPolicy {
    Off,
    Days7,
    Days30,
    Forever,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self::Days30
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionHistoryRecord {
    pub record_id: String,
    pub timestamp_utc_ms: u64,
    pub preview: String,
    /// Full final transcript. `#[serde(default)]` so records persisted before
    /// this field existed decrypt/parse as empty (UI falls back to preview).
    #[serde(default)]
    pub text: String,
    pub method: Option<InsertionMethod>,
    pub success: bool,
    pub source: String,
    pub message: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub starred: bool,
}

impl SessionHistoryRecord {
    /// Full transcript when available, falling back to the legacy preview
    /// for records persisted before `text` existed.
    fn display_text(&self) -> &str {
        if self.text.is_empty() {
            &self.preview
        } else {
            &self.text
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionHistoryQuery {
    pub limit: Option<usize>,
    pub include_failed: Option<bool>,
    pub search_query: Option<String>,
    pub tags: Option<Vec<String>>,
    pub starred: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEvent {
    pub action: String,
    pub policy: RetentionPolicy,
    pub retained_records: usize,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum HistoryExportPreset {
    Plain,
    MarkdownNotes,
    StudySummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryExportResult {
    pub preset: HistoryExportPreset,
    pub record_count: usize,
    pub content: String,
}

#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    #[error("failed to read history: {0}")]
    Read(std::io::Error),
    #[error("failed to write history: {0}")]
    Write(std::io::Error),
    #[error("failed to parse history JSON: {0}")]
    Parse(serde_json::Error),
    #[error("failed to encrypt history: {0}")]
    Encrypt(String),
    #[error("failed to decrypt history: {0}")]
    Decrypt(String),
    #[error("failed to decode history key: {0}")]
    KeyDecode(String),
    #[error("cannot resolve app data directory")]
    AppData,
    #[error("history record not found: {0}")]
    RecordNotFound(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct HistoryStore {
    retention_policy: RetentionPolicy,
    next_id: u64,
    records: Vec<SessionHistoryRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EncryptedHistoryStore {
    version: u8,
    nonce_b64: String,
    ciphertext_b64: String,
}

pub struct HistoryManager {
    path: PathBuf,
    _key_path: PathBuf,
    key: [u8; 32],
    store: HistoryStore,
}

impl HistoryManager {
    pub fn new() -> Result<Self, HistoryError> {
        let proj_dirs =
            ProjectDirs::from("com", "voicewave", "localcore").ok_or(HistoryError::AppData)?;
        let path = proj_dirs.config_dir().join("history.json");
        let key_path = proj_dirs.config_dir().join("history.key");
        Self::from_paths(path, key_path)
    }

    pub fn from_paths(
        path: impl AsRef<Path>,
        key_path: impl AsRef<Path>,
    ) -> Result<Self, HistoryError> {
        let path = path.as_ref().to_path_buf();
        let key_path = key_path.as_ref().to_path_buf();
        let key = load_or_create_key(&key_path)?;
        let mut manager = Self {
            path,
            _key_path: key_path,
            key,
            store: HistoryStore {
                retention_policy: RetentionPolicy::Days30,
                next_id: 1,
                records: Vec::new(),
            },
        };
        manager.load()?;
        let _ = manager.prune_expired();
        Ok(manager)
    }

    pub fn get_records(&self, query: SessionHistoryQuery) -> Vec<SessionHistoryRecord> {
        let include_failed = query.include_failed.unwrap_or(true);
        let limit = query.limit.unwrap_or(50).max(1);
        let search = query.search_query.unwrap_or_default().to_ascii_lowercase();
        let required_tags = query
            .tags
            .unwrap_or_default()
            .into_iter()
            .map(|tag| tag.trim().to_ascii_lowercase())
            .filter(|tag| !tag.is_empty())
            .collect::<Vec<_>>();
        let starred_filter = query.starred;

        self.store
            .records
            .iter()
            .rev()
            .filter(|row| include_failed || row.success)
            .filter(|row| {
                if let Some(required_starred) = starred_filter {
                    row.starred == required_starred
                } else {
                    true
                }
            })
            .filter(|row| {
                if search.is_empty() {
                    true
                } else {
                    row.preview.to_ascii_lowercase().contains(&search)
                        || row.text.to_ascii_lowercase().contains(&search)
                        || row.source.to_ascii_lowercase().contains(&search)
                        || row
                            .message
                            .as_deref()
                            .unwrap_or_default()
                            .to_ascii_lowercase()
                            .contains(&search)
                }
            })
            .filter(|row| {
                if required_tags.is_empty() {
                    return true;
                }
                let lower_tags = row
                    .tags
                    .iter()
                    .map(|tag| tag.to_ascii_lowercase())
                    .collect::<Vec<_>>();
                required_tags
                    .iter()
                    .all(|required| lower_tags.iter().any(|existing| existing == required))
            })
            .take(limit)
            .cloned()
            .collect()
    }

    pub fn record_insertion(
        &mut self,
        result: &InsertResult,
        text: &str,
    ) -> Result<(), HistoryError> {
        if self.store.retention_policy == RetentionPolicy::Off {
            return Ok(());
        }

        let record = SessionHistoryRecord {
            record_id: self.next_record_id(),
            timestamp_utc_ms: now_utc_ms(),
            preview: text.chars().take(140).collect(),
            text: text.to_string(),
            method: Some(result.method.clone()),
            success: result.success,
            source: "insertion".to_string(),
            message: result.message.clone(),
            tags: Vec::new(),
            starred: false,
        };
        self.append_record(record)
    }

    pub fn record_transcript(&mut self, transcript: &str) -> Result<(), HistoryError> {
        if self.store.retention_policy == RetentionPolicy::Off {
            return Ok(());
        }

        let record = SessionHistoryRecord {
            record_id: self.next_record_id(),
            timestamp_utc_ms: now_utc_ms(),
            preview: transcript.chars().take(140).collect(),
            text: transcript.to_string(),
            method: None,
            success: true,
            source: "dictation".to_string(),
            message: None,
            tags: Vec::new(),
            starred: false,
        };
        self.append_record(record)
    }

    /// Push a new record, enforce the count cap, prune expired records, then persist.
    fn append_record(&mut self, record: SessionHistoryRecord) -> Result<(), HistoryError> {
        self.store.records.push(record);
        if self.store.records.len() > MAX_HISTORY_RECORDS {
            let keep_from = self.store.records.len() - MAX_HISTORY_RECORDS;
            self.store.records = self.store.records.split_off(keep_from);
        }
        self.prune_expired()?;
        self.persist()
    }

    pub fn tag_record(
        &mut self,
        record_id: &str,
        tag: &str,
    ) -> Result<SessionHistoryRecord, HistoryError> {
        let normalized_tag = tag.trim();
        if normalized_tag.is_empty() {
            return self
                .store
                .records
                .iter()
                .find(|row| row.record_id == record_id)
                .cloned()
                .ok_or_else(|| HistoryError::RecordNotFound(record_id.to_string()));
        }

        let row = self
            .store
            .records
            .iter_mut()
            .find(|row| row.record_id == record_id)
            .ok_or_else(|| HistoryError::RecordNotFound(record_id.to_string()))?;

        if !row
            .tags
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(normalized_tag))
        {
            row.tags.push(normalized_tag.to_string());
        }
        let updated = row.clone();
        self.persist()?;
        Ok(updated)
    }

    pub fn toggle_star_record(
        &mut self,
        record_id: &str,
        starred: bool,
    ) -> Result<SessionHistoryRecord, HistoryError> {
        let row = self
            .store
            .records
            .iter_mut()
            .find(|row| row.record_id == record_id)
            .ok_or_else(|| HistoryError::RecordNotFound(record_id.to_string()))?;
        row.starred = starred;
        let updated = row.clone();
        self.persist()?;
        Ok(updated)
    }

    pub fn export_preset(
        &self,
        preset: HistoryExportPreset,
        query: SessionHistoryQuery,
    ) -> HistoryExportResult {
        let records = self.get_records(query);
        let content = match preset {
            HistoryExportPreset::Plain => records
                .iter()
                .map(|row| {
                    format!(
                        "[{}] {}{}",
                        row.timestamp_utc_ms,
                        row.display_text(),
                        if row.starred { " ?" } else { "" }
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
            HistoryExportPreset::MarkdownNotes => records
                .iter()
                .map(|row| {
                    let tags = if row.tags.is_empty() {
                        String::new()
                    } else {
                        format!(" _(#{})_", row.tags.join(" #"))
                    };
                    format!("- **{}**: {}{}", row.source, row.display_text(), tags)
                })
                .collect::<Vec<_>>()
                .join("\n"),
            HistoryExportPreset::StudySummary => {
                let total = records.len();
                let starred = records.iter().filter(|row| row.starred).count();
                let top_sources = ["dictation", "insertion"]
                    .iter()
                    .map(|source| {
                        let count = records.iter().filter(|row| row.source == *source).count();
                        format!("- {}: {}", source, count)
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                format!(
                    "Study Summary\nTotal Records: {total}\nStarred: {starred}\nSource Mix:\n{top_sources}"
                )
            }
        };

        HistoryExportResult {
            preset,
            record_count: records.len(),
            content,
        }
    }

    pub fn set_retention_policy(
        &mut self,
        policy: RetentionPolicy,
    ) -> Result<RetentionPolicy, HistoryError> {
        self.store.retention_policy = policy.clone();
        self.prune_expired()?;
        self.persist()?;
        Ok(policy)
    }

    pub fn retention_policy(&self) -> RetentionPolicy {
        self.store.retention_policy.clone()
    }

    pub fn prune_now(&mut self) -> Result<usize, HistoryError> {
        let before = self.store.records.len();
        self.prune_expired()?;
        self.persist()?;
        Ok(before.saturating_sub(self.store.records.len()))
    }

    pub fn clear(&mut self) -> Result<usize, HistoryError> {
        let removed = self.store.records.len();
        self.store.records.clear();
        self.persist()?;
        Ok(removed)
    }

    pub fn event(&self, action: &str, message: Option<String>) -> HistoryEvent {
        HistoryEvent {
            action: action.to_string(),
            policy: self.retention_policy(),
            retained_records: self.store.records.len(),
            message,
        }
    }

    fn next_record_id(&mut self) -> String {
        let id = self.store.next_id;
        self.store.next_id += 1;
        format!("hist-{id}")
    }

    fn prune_expired(&mut self) -> Result<(), HistoryError> {
        match self.store.retention_policy {
            RetentionPolicy::Off => {
                self.store.records.clear();
            }
            RetentionPolicy::Days7 => {
                let cutoff = now_utc_ms().saturating_sub(7 * 24 * 60 * 60 * 1000);
                self.store
                    .records
                    .retain(|row| row.timestamp_utc_ms >= cutoff);
            }
            RetentionPolicy::Days30 => {
                let cutoff = now_utc_ms().saturating_sub(30 * 24 * 60 * 60 * 1000);
                self.store
                    .records
                    .retain(|row| row.timestamp_utc_ms >= cutoff);
            }
            RetentionPolicy::Forever => {}
        }
        if self.store.records.len() > 1_000 {
            let keep_from = self.store.records.len() - 1_000;
            self.store.records = self.store.records.split_off(keep_from);
        }
        Ok(())
    }

    fn load(&mut self) -> Result<(), HistoryError> {
        if !self.path.exists() {
            return Ok(());
        }
        let raw = fs::read_to_string(&self.path).map_err(HistoryError::Read)?;
        if let Ok(encrypted) = serde_json::from_str::<EncryptedHistoryStore>(&raw) {
            self.store = decrypt_history_store(&encrypted, &self.key)?;
            return Ok(());
        }

        self.store = serde_json::from_str(&raw).map_err(HistoryError::Parse)?;
        self.persist()?;
        Ok(())
    }

    fn persist(&self) -> Result<(), HistoryError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(HistoryError::Write)?;
        }
        let encrypted = encrypt_history_store(&self.store, &self.key)?;
        let raw = serde_json::to_string_pretty(&encrypted).map_err(HistoryError::Parse)?;
        fs::write(&self.path, raw).map_err(HistoryError::Write)?;
        Ok(())
    }
}

fn now_utc_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

fn load_or_create_key(path: &PathBuf) -> Result<[u8; 32], HistoryError> {
    if path.exists() {
        let encoded = fs::read_to_string(path).map_err(HistoryError::Read)?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded.trim())
            .map_err(|err| HistoryError::KeyDecode(err.to_string()))?;
        if bytes.len() != 32 {
            return Err(HistoryError::KeyDecode(
                "history.key must decode to 32 bytes".to_string(),
            ));
        }
        let mut key = [0_u8; 32];
        key.copy_from_slice(&bytes);
        return Ok(key);
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(HistoryError::Write)?;
    }
    let mut key = [0_u8; 32];
    OsRng.fill_bytes(&mut key);
    let encoded = base64::engine::general_purpose::STANDARD.encode(key);
    fs::write(path, encoded).map_err(HistoryError::Write)?;
    Ok(key)
}

fn encrypt_history_store(
    store: &HistoryStore,
    key: &[u8; 32],
) -> Result<EncryptedHistoryStore, HistoryError> {
    let cipher =
        Aes256Gcm::new_from_slice(key).map_err(|err| HistoryError::Encrypt(err.to_string()))?;
    let mut nonce_bytes = [0_u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let plaintext = serde_json::to_vec(store).map_err(HistoryError::Parse)?;
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_ref())
        .map_err(|err| HistoryError::Encrypt(err.to_string()))?;

    Ok(EncryptedHistoryStore {
        version: 1,
        nonce_b64: base64::engine::general_purpose::STANDARD.encode(nonce_bytes),
        ciphertext_b64: base64::engine::general_purpose::STANDARD.encode(ciphertext),
    })
}

fn decrypt_history_store(
    encrypted: &EncryptedHistoryStore,
    key: &[u8; 32],
) -> Result<HistoryStore, HistoryError> {
    if encrypted.version != 1 {
        return Err(HistoryError::Decrypt(format!(
            "unsupported history encryption version {}",
            encrypted.version
        )));
    }

    let nonce_bytes = base64::engine::general_purpose::STANDARD
        .decode(encrypted.nonce_b64.as_bytes())
        .map_err(|err| HistoryError::Decrypt(err.to_string()))?;
    let ciphertext = base64::engine::general_purpose::STANDARD
        .decode(encrypted.ciphertext_b64.as_bytes())
        .map_err(|err| HistoryError::Decrypt(err.to_string()))?;
    if nonce_bytes.len() != 12 {
        return Err(HistoryError::Decrypt("nonce must be 12 bytes".to_string()));
    }

    let cipher =
        Aes256Gcm::new_from_slice(key).map_err(|err| HistoryError::Decrypt(err.to_string()))?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext.as_ref())
        .map_err(|err| HistoryError::Decrypt(err.to_string()))?;
    serde_json::from_slice(&plaintext).map_err(HistoryError::Parse)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retention_off_drops_records() {
        let key_path = std::env::temp_dir().join("voicewave-history-test.key");
        let key = load_or_create_key(&key_path).expect("key");
        let mut manager = HistoryManager {
            path: std::env::temp_dir().join("voicewave-history-test.json"),
            _key_path: key_path,
            key,
            store: HistoryStore::default(),
        };
        manager.store.retention_policy = RetentionPolicy::Forever;
        manager.store.records.push(SessionHistoryRecord {
            record_id: "hist-1".to_string(),
            timestamp_utc_ms: now_utc_ms(),
            preview: "hello".to_string(),
            text: "hello".to_string(),
            method: None,
            success: true,
            source: "dictation".to_string(),
            message: None,
            tags: vec![],
            starred: false,
        });

        let _ = manager.set_retention_policy(RetentionPolicy::Off);
        assert!(manager.store.records.is_empty());
    }

    #[test]
    fn persisted_history_is_encrypted() {
        let temp =
            std::env::temp_dir().join(format!("voicewave-history-encrypted-{}.json", now_utc_ms()));
        let key_path =
            std::env::temp_dir().join(format!("voicewave-history-encrypted-{}.key", now_utc_ms()));
        let key = load_or_create_key(&key_path).expect("key");
        let mut manager = HistoryManager {
            path: temp.clone(),
            _key_path: key_path,
            key,
            store: HistoryStore::default(),
        };
        manager.store.retention_policy = RetentionPolicy::Forever;
        manager
            .record_transcript("secret phrase should not be plaintext")
            .expect("persist encrypted");

        let raw = fs::read_to_string(temp).expect("read persisted history");
        assert!(!raw.contains("secret phrase should not be plaintext"));
        assert!(raw.contains("ciphertextB64"));
    }

    #[test]
    fn record_stores_full_text_and_truncated_preview() {
        let key_path =
            std::env::temp_dir().join(format!("voicewave-history-fulltext-{}.key", now_utc_ms()));
        let key = load_or_create_key(&key_path).expect("key");
        let mut manager = HistoryManager {
            path: std::env::temp_dir()
                .join(format!("voicewave-history-fulltext-{}.json", now_utc_ms())),
            _key_path: key_path,
            key,
            store: HistoryStore::default(),
        };
        manager.store.retention_policy = RetentionPolicy::Forever;

        let long_text: String = "abcdefghij".repeat(20); // 200 chars
        manager
            .record_transcript(&long_text)
            .expect("record transcript");

        let stored = manager.store.records.last().expect("record present");
        assert_eq!(stored.text, long_text);
        assert_eq!(
            stored.preview,
            long_text.chars().take(140).collect::<String>()
        );
        assert_eq!(stored.preview.chars().count(), 140);
    }

    #[test]
    fn history_caps_at_max_records() {
        let key_path =
            std::env::temp_dir().join(format!("voicewave-history-cap-{}.key", now_utc_ms()));
        let key = load_or_create_key(&key_path).expect("key");
        let mut manager = HistoryManager {
            path: std::env::temp_dir().join(format!("voicewave-history-cap-{}.json", now_utc_ms())),
            _key_path: key_path,
            key,
            store: HistoryStore::default(),
        };
        manager.store.retention_policy = RetentionPolicy::Forever;

        let total = MAX_HISTORY_RECORDS + 10;
        for i in 0..total {
            manager
                .record_transcript(&format!("record number {i}"))
                .expect("record transcript");
        }

        assert_eq!(manager.store.records.len(), MAX_HISTORY_RECORDS);
        let oldest_survivor_index = total - MAX_HISTORY_RECORDS;
        assert_eq!(
            manager.store.records.first().expect("first record").text,
            format!("record number {oldest_survivor_index}")
        );
        assert_eq!(
            manager.store.records.last().expect("last record").text,
            format!("record number {}", total - 1)
        );
    }

    #[test]
    fn legacy_record_without_text_field_parses() {
        let json = r#"{
            "recordId": "hist-1",
            "timestampUtcMs": 123,
            "preview": "hello world",
            "method": null,
            "success": true,
            "source": "dictation",
            "message": null
        }"#;

        let record: SessionHistoryRecord =
            serde_json::from_str(json).expect("legacy record without text should parse");
        assert_eq!(record.text, "");
        assert_eq!(record.preview, "hello world");
    }
}
