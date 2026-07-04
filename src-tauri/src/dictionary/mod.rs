use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use base64::Engine;
use directories::ProjectDirs;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryQueueItem {
    pub entry_id: String,
    pub term: String,
    pub source_preview: String,
    pub created_at_utc_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryTerm {
    pub term_id: String,
    pub term: String,
    pub source: String,
    pub created_at_utc_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryEvent {
    pub action: String,
    pub queue_size: usize,
    pub term_count: usize,
    pub message: Option<String>,
}

/// Portable, versioned envelope for exporting/importing approved dictionary
/// terms. This is the interchange schema (v1) — see maintenance notes in
/// `plans/001-dictionary-export-import.md`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryExport {
    pub version: u8,
    pub exported_at_utc_ms: u64,
    pub terms: Vec<DictionaryTerm>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryImportSummary {
    pub added: usize,
    pub skipped: usize,
    pub total_in_file: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum DictionaryError {
    #[error("failed to read dictionary: {0}")]
    Read(std::io::Error),
    #[error("failed to write dictionary: {0}")]
    Write(std::io::Error),
    #[error("failed to parse dictionary JSON: {0}")]
    Parse(serde_json::Error),
    #[error("failed to encrypt dictionary: {0}")]
    Encrypt(String),
    #[error("failed to decrypt dictionary: {0}")]
    Decrypt(String),
    #[error("failed to decode dictionary key: {0}")]
    KeyDecode(String),
    #[error("cannot resolve app data directory")]
    AppData,
    #[error("dictionary queue entry not found: {0}")]
    QueueEntryNotFound(String),
    #[error("dictionary term not found: {0}")]
    TermNotFound(String),
    #[error("dictionary term is empty")]
    EmptyTerm,
    #[error("unsupported dictionary import version {0}")]
    UnsupportedImportVersion(u8),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct DictionaryStore {
    next_id: u64,
    queue: Vec<DictionaryQueueItem>,
    terms: Vec<DictionaryTerm>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EncryptedDictionaryStore {
    version: u8,
    nonce_b64: String,
    ciphertext_b64: String,
}

pub struct DictionaryManager {
    path: PathBuf,
    _key_path: PathBuf,
    key: [u8; 32],
    store: DictionaryStore,
}

impl DictionaryManager {
    pub fn new() -> Result<Self, DictionaryError> {
        let proj_dirs =
            ProjectDirs::from("com", "voicewave", "localcore").ok_or(DictionaryError::AppData)?;
        let path = proj_dirs.config_dir().join("dictionary.json");
        let key_path = proj_dirs.config_dir().join("dictionary.key");
        Self::from_paths(path, key_path)
    }

    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, DictionaryError> {
        let path = path.as_ref().to_path_buf();
        let key_path = path.with_extension("key");
        Self::from_paths(path, key_path)
    }

    pub fn from_paths(
        path: impl AsRef<Path>,
        key_path: impl AsRef<Path>,
    ) -> Result<Self, DictionaryError> {
        let path = path.as_ref().to_path_buf();
        let key_path = key_path.as_ref().to_path_buf();
        let key = load_or_create_key(&key_path)?;
        let mut manager = Self {
            path,
            _key_path: key_path,
            key,
            store: DictionaryStore {
                next_id: 1,
                queue: Vec::new(),
                terms: Vec::new(),
            },
        };
        manager.load()?;
        Ok(manager)
    }

    pub fn ingest_transcript(&mut self, transcript: &str) -> Result<usize, DictionaryError> {
        self.ingest_transcript_with_signal(transcript, false)
    }

    pub fn ingest_transcript_with_signal(
        &mut self,
        transcript: &str,
        low_confidence: bool,
    ) -> Result<usize, DictionaryError> {
        let preview = transcript.chars().take(80).collect::<String>();
        let mut added = 0usize;

        for candidate in candidate_terms(transcript, low_confidence)
            .into_iter()
            .take(3)
        {
            if self.contains_term(&candidate) || self.in_queue(&candidate) {
                continue;
            }

            let entry_id = self.next_id("dq");
            self.store.queue.push(DictionaryQueueItem {
                entry_id,
                term: candidate,
                source_preview: preview.clone(),
                created_at_utc_ms: now_utc_ms(),
            });
            added += 1;
        }

        if added > 0 {
            self.persist()?;
        }
        Ok(added)
    }

    pub fn queue_correction_candidates(
        &mut self,
        candidates: &[String],
        source_preview: &str,
    ) -> Result<usize, DictionaryError> {
        let mut added = 0usize;
        let preview = source_preview.chars().take(80).collect::<String>();

        for candidate in candidates.iter().take(3) {
            let normalized = candidate.trim();
            if normalized.is_empty() || !is_high_signal_term(normalized) {
                continue;
            }
            if self.contains_term(normalized) || self.in_queue(normalized) {
                continue;
            }

            let entry_id = self.next_id("dq");
            self.store.queue.push(DictionaryQueueItem {
                entry_id,
                term: normalized.to_string(),
                source_preview: preview.clone(),
                created_at_utc_ms: now_utc_ms(),
            });
            added += 1;
        }

        if added > 0 {
            self.persist()?;
        }
        Ok(added)
    }

    pub fn get_queue(&self, limit: Option<usize>) -> Vec<DictionaryQueueItem> {
        let take = limit.unwrap_or(50).max(1);
        self.store.queue.iter().rev().take(take).cloned().collect()
    }

    pub fn approve_entry(
        &mut self,
        entry_id: &str,
        normalized_text: Option<String>,
    ) -> Result<DictionaryTerm, DictionaryError> {
        let idx = self
            .store
            .queue
            .iter()
            .position(|entry| entry.entry_id == entry_id)
            .ok_or_else(|| DictionaryError::QueueEntryNotFound(entry_id.to_string()))?;
        let entry = self.store.queue.remove(idx);

        let term = DictionaryTerm {
            term_id: self.next_id("dt"),
            term: normalized_text
                .unwrap_or_else(|| entry.term)
                .trim()
                .to_string(),
            source: "queue-approval".to_string(),
            created_at_utc_ms: now_utc_ms(),
        };
        self.store.terms.push(term.clone());
        self.persist()?;
        Ok(term)
    }

    pub fn reject_entry(
        &mut self,
        entry_id: &str,
        reason: Option<String>,
    ) -> Result<(), DictionaryError> {
        let idx = self
            .store
            .queue
            .iter()
            .position(|entry| entry.entry_id == entry_id)
            .ok_or_else(|| DictionaryError::QueueEntryNotFound(entry_id.to_string()))?;
        self.store.queue.remove(idx);
        if reason.as_deref().is_some() {
            // Reason is currently included for audit compatibility, but not persisted in Phase III.
        }
        self.persist()?;
        Ok(())
    }

    pub fn get_terms(&self, query: Option<String>) -> Vec<DictionaryTerm> {
        let query = query.unwrap_or_default().trim().to_ascii_lowercase();

        let mut rows: Vec<_> = self
            .store
            .terms
            .iter()
            .filter(|term| query.is_empty() || term.term.to_ascii_lowercase().contains(&query))
            .cloned()
            .collect();
        rows.sort_by_key(|row| row.created_at_utc_ms);
        rows
    }

    pub fn remove_term(&mut self, term_id: &str) -> Result<(), DictionaryError> {
        let idx = self
            .store
            .terms
            .iter()
            .position(|term| term.term_id == term_id)
            .ok_or_else(|| DictionaryError::TermNotFound(term_id.to_string()))?;
        self.store.terms.remove(idx);
        self.persist()?;
        Ok(())
    }

    pub fn add_term(
        &mut self,
        term: &str,
        source: Option<String>,
    ) -> Result<DictionaryTerm, DictionaryError> {
        let normalized = term.trim();
        if normalized.is_empty() {
            return Err(DictionaryError::EmptyTerm);
        }

        if let Some(existing) = self
            .store
            .terms
            .iter()
            .find(|row| row.term.eq_ignore_ascii_case(normalized))
            .cloned()
        {
            return Ok(existing);
        }

        // If the term existed in pending queue, remove it now that user added it explicitly.
        self.store
            .queue
            .retain(|row| !row.term.eq_ignore_ascii_case(normalized));

        let added = DictionaryTerm {
            term_id: self.next_id("dt"),
            term: normalized.to_string(),
            source: source.unwrap_or_else(|| "manual-add".to_string()),
            created_at_utc_ms: now_utc_ms(),
        };
        self.store.terms.push(added.clone());

        self.persist()?;

        Ok(added)
    }

    /// Snapshot all approved terms into a portable, versioned envelope. The
    /// frontend serializes this to JSON and saves the file — the manager never
    /// writes to arbitrary paths.
    pub fn export_terms(&self) -> DictionaryExport {
        DictionaryExport {
            version: 1,
            exported_at_utc_ms: now_utc_ms(),
            terms: self.store.terms.clone(),
        }
    }

    /// Parse an exported envelope and merge its approved terms into the store,
    /// reusing `add_term`'s case-insensitive dedupe. Returns a summary of how
    /// many terms were newly added versus skipped as duplicates. Parse and
    /// version errors are surfaced (never panics on malformed input).
    pub fn import_terms(
        &mut self,
        payload: &str,
    ) -> Result<DictionaryImportSummary, DictionaryError> {
        let export: DictionaryExport =
            serde_json::from_str(payload).map_err(DictionaryError::Parse)?;
        if export.version != 1 {
            return Err(DictionaryError::UnsupportedImportVersion(export.version));
        }

        let total_in_file = export.terms.len();
        let mut added = 0usize;
        let mut skipped = 0usize;

        for term in &export.terms {
            let normalized = term.term.trim();
            // Skip empties and existing terms as duplicates; `contains_term`
            // classifies added-vs-skipped BEFORE `add_term` (which returns the
            // existing term on a dupe without signalling whether it was new).
            if normalized.is_empty() || self.contains_term(normalized) {
                skipped += 1;
                continue;
            }
            self.add_term(normalized, Some("import".to_string()))?;
            added += 1;
        }

        Ok(DictionaryImportSummary {
            added,
            skipped,
            total_in_file,
        })
    }

    pub fn event(&self, action: &str, message: Option<String>) -> DictionaryEvent {
        DictionaryEvent {
            action: action.to_string(),
            queue_size: self.store.queue.len(),
            term_count: self.store.terms.len(),
            message,
        }
    }

    fn contains_term(&self, candidate: &str) -> bool {
        let candidate = candidate.to_ascii_lowercase();
        self.store
            .terms
            .iter()
            .any(|term| term.term.to_ascii_lowercase() == candidate)
    }

    fn in_queue(&self, candidate: &str) -> bool {
        let candidate = candidate.to_ascii_lowercase();
        self.store
            .queue
            .iter()
            .any(|item| item.term.to_ascii_lowercase() == candidate)
    }

    fn next_id(&mut self, prefix: &str) -> String {
        let id = self.store.next_id;
        self.store.next_id += 1;
        format!("{prefix}-{id}")
    }

    fn load(&mut self) -> Result<(), DictionaryError> {
        if !self.path.exists() {
            return Ok(());
        }
        let raw = fs::read_to_string(&self.path).map_err(DictionaryError::Read)?;
        if let Ok(encrypted) = serde_json::from_str::<EncryptedDictionaryStore>(&raw) {
            self.store = decrypt_dictionary_store(&encrypted, &self.key)?;
        } else {
            self.store = serde_json::from_str(&raw).map_err(DictionaryError::Parse)?;
            backup_legacy_plaintext(&self.path)?;
            self.persist()?;
        }
        let before = self.store.queue.len();
        self.store
            .queue
            .retain(|item| is_high_signal_term(&item.term));
        if self.store.queue.len() != before {
            self.persist()?;
        }
        Ok(())
    }

    fn persist(&self) -> Result<(), DictionaryError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(DictionaryError::Write)?;
        }
        let encrypted = encrypt_dictionary_store(&self.store, &self.key)?;
        let raw = serde_json::to_string_pretty(&encrypted).map_err(DictionaryError::Parse)?;
        fs::write(&self.path, raw).map_err(DictionaryError::Write)?;
        Ok(())
    }
}

fn backup_legacy_plaintext(path: &Path) -> Result<(), DictionaryError> {
    if !path.exists() {
        return Ok(());
    }
    let backup_path = path.with_extension("json.bak");
    if backup_path.exists() {
        return Ok(());
    }
    fs::copy(path, backup_path).map_err(DictionaryError::Write)?;
    Ok(())
}

fn load_or_create_key(path: &PathBuf) -> Result<[u8; 32], DictionaryError> {
    if path.exists() {
        let encoded = fs::read_to_string(path).map_err(DictionaryError::Read)?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded.trim())
            .map_err(|err| DictionaryError::KeyDecode(err.to_string()))?;
        if bytes.len() != 32 {
            return Err(DictionaryError::KeyDecode(
                "dictionary.key must decode to 32 bytes".to_string(),
            ));
        }
        let mut key = [0_u8; 32];
        key.copy_from_slice(&bytes);
        return Ok(key);
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(DictionaryError::Write)?;
    }
    let mut key = [0_u8; 32];
    OsRng.fill_bytes(&mut key);
    let encoded = base64::engine::general_purpose::STANDARD.encode(key);
    fs::write(path, encoded).map_err(DictionaryError::Write)?;
    Ok(key)
}

fn encrypt_dictionary_store(
    store: &DictionaryStore,
    key: &[u8; 32],
) -> Result<EncryptedDictionaryStore, DictionaryError> {
    let cipher =
        Aes256Gcm::new_from_slice(key).map_err(|err| DictionaryError::Encrypt(err.to_string()))?;
    let mut nonce_bytes = [0_u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let plaintext = serde_json::to_vec(store).map_err(DictionaryError::Parse)?;
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_ref())
        .map_err(|err| DictionaryError::Encrypt(err.to_string()))?;

    Ok(EncryptedDictionaryStore {
        version: 1,
        nonce_b64: base64::engine::general_purpose::STANDARD.encode(nonce_bytes),
        ciphertext_b64: base64::engine::general_purpose::STANDARD.encode(ciphertext),
    })
}

fn decrypt_dictionary_store(
    encrypted: &EncryptedDictionaryStore,
    key: &[u8; 32],
) -> Result<DictionaryStore, DictionaryError> {
    if encrypted.version != 1 {
        return Err(DictionaryError::Decrypt(format!(
            "unsupported dictionary encryption version {}",
            encrypted.version
        )));
    }

    let nonce_bytes = base64::engine::general_purpose::STANDARD
        .decode(encrypted.nonce_b64.as_bytes())
        .map_err(|err| DictionaryError::Decrypt(err.to_string()))?;
    let ciphertext = base64::engine::general_purpose::STANDARD
        .decode(encrypted.ciphertext_b64.as_bytes())
        .map_err(|err| DictionaryError::Decrypt(err.to_string()))?;
    if nonce_bytes.len() != 12 {
        return Err(DictionaryError::Decrypt(
            "dictionary nonce must be 12 bytes".to_string(),
        ));
    }

    let cipher =
        Aes256Gcm::new_from_slice(key).map_err(|err| DictionaryError::Decrypt(err.to_string()))?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext.as_ref())
        .map_err(|err| DictionaryError::Decrypt(err.to_string()))?;
    serde_json::from_slice(&plaintext).map_err(DictionaryError::Parse)
}

fn candidate_terms(transcript: &str, low_confidence: bool) -> Vec<String> {
    if !low_confidence {
        return Vec::new();
    }

    let mut seen = HashSet::new();
    let mut candidates = Vec::new();

    for token in transcript.split_whitespace() {
        let cleaned = token.trim_matches(|ch: char| {
            !ch.is_ascii_alphanumeric() && ch != '-' && ch != '_' && ch != '.'
        });
        if cleaned.len() < 4 || cleaned.len() > 36 {
            continue;
        }
        let normalized_key = cleaned.to_ascii_lowercase();
        if seen.contains(&normalized_key) {
            continue;
        }

        if is_high_signal_term(cleaned) {
            seen.insert(normalized_key);
            candidates.push(cleaned.to_string());
        }
    }

    candidates
}

fn is_high_signal_term(token: &str) -> bool {
    let has_digit = token.chars().any(|ch| ch.is_ascii_digit());
    let has_structure =
        has_digit || token.contains('-') || token.contains('_') || token.contains('.');
    if has_structure {
        return token.len() >= 4;
    }

    let uppercase_count = token.chars().filter(|ch| ch.is_ascii_uppercase()).count();
    let has_internal_upper = token.chars().skip(1).any(|ch| ch.is_ascii_uppercase());

    if uppercase_count >= 3 {
        return token.len() >= 4;
    }
    if has_internal_upper {
        return token.len() >= 5;
    }
    if uppercase_count >= 2 && token.len() >= 6 {
        return true;
    }

    false
}

fn now_utc_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_manager(name: &str) -> DictionaryManager {
        let base = std::env::temp_dir().join(format!("voicewave-dictionary-{name}-{}", now_utc_ms()));
        let path = base.with_extension("json");
        let key_path = base.with_extension("key");
        let key = load_or_create_key(&key_path).expect("key");
        DictionaryManager {
            path,
            _key_path: key_path,
            key,
            store: DictionaryStore::default(),
        }
    }

    #[test]
    fn ingest_adds_distinctive_candidates() {
        let mut manager = test_manager("distinctive");

        let added = manager
            .ingest_transcript_with_signal("Reviewed VoiceWave FW-V3 roadmap for OpenAI", true)
            .expect("ingest should succeed");
        assert!(added > 0);
        assert!(!manager.get_queue(None).is_empty());
    }

    #[test]
    fn ingest_ignores_plain_sentence_words() {
        let mut manager = test_manager("plain");

        let added = manager
            .ingest_transcript("Today we discussed the project and the workflow in detail")
            .expect("ingest should succeed");
        assert_eq!(added, 0);
    }

    #[test]
    fn ingest_requires_low_confidence_signal() {
        let mut manager = test_manager("signal");

        let added = manager
            .ingest_transcript_with_signal("Reviewed VoiceWave FW-V3 roadmap for OpenAI", false)
            .expect("ingest should succeed");
        assert_eq!(added, 0);
    }

    #[test]
    fn persisted_dictionary_is_encrypted() {
        let mut manager = test_manager("encrypted");
        manager
            .add_term("VoiceWave-v3", Some("unit-test".to_string()))
            .expect("add term");

        let raw = fs::read_to_string(&manager.path).expect("read dictionary");
        assert!(!raw.contains("VoiceWave-v3"));
        assert!(raw.contains("ciphertextB64"));
    }

    #[test]
    fn export_then_import_round_trips() {
        let mut manager = test_manager("export-roundtrip");
        manager.add_term("VoiceWave", None).expect("add term");
        manager.add_term("Tauri", None).expect("add term");
        manager.add_term("Whisper", None).expect("add term");

        let export = manager.export_terms();
        assert_eq!(export.version, 1);
        assert_eq!(export.terms.len(), 3);
        let payload = serde_json::to_string(&export).expect("serialize export");

        let mut fresh = test_manager("export-roundtrip-fresh");
        let summary = fresh.import_terms(&payload).expect("import should succeed");
        assert_eq!(summary.added, 3);
        assert_eq!(summary.skipped, 0);
        assert_eq!(summary.total_in_file, 3);
        assert_eq!(fresh.get_terms(None).len(), 3);
    }

    #[test]
    fn import_skips_duplicates() {
        let mut manager = test_manager("import-dupes");
        manager.add_term("Alpha", None).expect("add term");
        manager.add_term("Beta", None).expect("add term");

        let export = DictionaryExport {
            version: 1,
            exported_at_utc_ms: now_utc_ms(),
            terms: vec![
                DictionaryTerm {
                    term_id: "dt-1".to_string(),
                    term: "Beta".to_string(),
                    source: "import".to_string(),
                    created_at_utc_ms: 0,
                },
                DictionaryTerm {
                    term_id: "dt-2".to_string(),
                    term: "Gamma".to_string(),
                    source: "import".to_string(),
                    created_at_utc_ms: 0,
                },
            ],
        };
        let payload = serde_json::to_string(&export).expect("serialize export");

        let summary = manager.import_terms(&payload).expect("import should succeed");
        assert_eq!(summary.added, 1);
        assert_eq!(summary.skipped, 1);
        assert_eq!(summary.total_in_file, 2);
    }

    #[test]
    fn import_rejects_unsupported_version() {
        let mut manager = test_manager("import-version");
        let payload = r#"{"version":99,"exportedAtUtcMs":0,"terms":[]}"#;
        assert!(manager.import_terms(payload).is_err());
    }

    #[test]
    fn import_rejects_malformed_json() {
        let mut manager = test_manager("import-malformed");
        assert!(manager.import_terms("{not json").is_err());
    }
}
