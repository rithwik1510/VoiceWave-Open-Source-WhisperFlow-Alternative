use crate::secure_store::{
    decrypt_bytes, encrypt_json, load_or_create_key, EncryptedEnvelope, SecureStoreError,
};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use unicode_normalization::UnicodeNormalization;

const MAX_APPROVED_TERMS: usize = 1000;
const MAX_PENDING_TERMS: usize = 50;
const MAX_TERM_CHARS: usize = 72;

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
pub struct DictionarySyncRecord {
    pub term: String,
    pub normalized_term: String,
    pub source: String,
    pub created_at_utc_ms: u64,
    pub updated_at_utc_ms: u64,
    pub deleted_at_utc_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryReconcileResult {
    pub terms: Vec<DictionaryTerm>,
    pub records: Vec<DictionarySyncRecord>,
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
    #[error("dictionary term exceeds {MAX_TERM_CHARS} characters")]
    TermTooLong,
    #[error("dictionary term contains control characters or line breaks")]
    InvalidTermCharacters,
    #[error("dictionary already contains the maximum of {MAX_APPROVED_TERMS} active terms")]
    ApprovedTermLimit,
    #[error("dictionary sync record identity does not match its normalized term")]
    InvalidSyncIdentity,
    #[error("unsupported dictionary import version {0}")]
    UnsupportedImportVersion(u8),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct StoredDictionaryRecord {
    term_id: String,
    term: String,
    normalized_term: String,
    source: String,
    created_at_utc_ms: u64,
    updated_at_utc_ms: u64,
    deleted_at_utc_ms: Option<u64>,
}

impl StoredDictionaryRecord {
    fn public_term(&self) -> Option<DictionaryTerm> {
        self.deleted_at_utc_ms.is_none().then(|| DictionaryTerm {
            term_id: self.term_id.clone(),
            term: self.term.clone(),
            source: self.source.clone(),
            created_at_utc_ms: self.created_at_utc_ms,
        })
    }

    fn sync_record(&self) -> DictionarySyncRecord {
        DictionarySyncRecord {
            term: self.term.clone(),
            normalized_term: self.normalized_term.clone(),
            source: self.source.clone(),
            created_at_utc_ms: self.created_at_utc_ms,
            updated_at_utc_ms: self.updated_at_utc_ms,
            deleted_at_utc_ms: self.deleted_at_utc_ms,
        }
    }

    fn effective_timestamp(&self) -> u64 {
        self.deleted_at_utc_ms
            .unwrap_or(self.updated_at_utc_ms)
            .max(self.updated_at_utc_ms)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DictionaryStore {
    next_id: u64,
    queue: Vec<DictionaryQueueItem>,
    terms: Vec<StoredDictionaryRecord>,
}

impl Default for DictionaryStore {
    fn default() -> Self {
        Self {
            next_id: 1,
            queue: Vec::new(),
            terms: Vec::new(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CurrentDictionaryStore {
    #[serde(default = "default_next_id")]
    next_id: u64,
    #[serde(default)]
    queue: Vec<DictionaryQueueItem>,
    #[serde(default)]
    terms: Vec<StoredDictionaryRecord>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyDictionaryStore {
    #[serde(default = "default_next_id")]
    next_id: u64,
    #[serde(default)]
    queue: Vec<DictionaryQueueItem>,
    #[serde(default)]
    terms: Vec<DictionaryTerm>,
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
        let key = load_or_create_key(&key_path, "dictionary").map_err(map_secure_store_error)?;
        let mut manager = Self {
            path,
            _key_path: key_path,
            key,
            store: DictionaryStore::default(),
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
            self.cap_pending_queue();
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
            self.cap_pending_queue();
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
        let candidate = normalized_text.unwrap_or_else(|| self.store.queue[idx].term.clone());
        let (display, identity) = normalize_and_validate_term(&candidate)?;
        let term = self.upsert_active_record(
            display,
            identity,
            "queue-approval".to_string(),
            now_utc_ms(),
        )?;
        self.store.queue.remove(idx);
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
        let query = normalize_identity_lossy(&query.unwrap_or_default());

        let mut rows: Vec<_> = self
            .store
            .terms
            .iter()
            .filter(|term| {
                term.deleted_at_utc_ms.is_none()
                    && (query.is_empty() || term.normalized_term.contains(&query))
            })
            .filter_map(StoredDictionaryRecord::public_term)
            .collect();
        rows.sort_by_key(|row| row.created_at_utc_ms);
        rows
    }

    pub fn remove_term(&mut self, term_id: &str) -> Result<(), DictionaryError> {
        let record = self
            .store
            .terms
            .iter_mut()
            .find(|term| term.term_id == term_id && term.deleted_at_utc_ms.is_none())
            .ok_or_else(|| DictionaryError::TermNotFound(term_id.to_string()))?;
        let timestamp = monotonic_timestamp(record.updated_at_utc_ms);
        record.updated_at_utc_ms = timestamp;
        record.deleted_at_utc_ms = Some(timestamp);
        self.persist()?;
        Ok(())
    }

    pub fn add_term(
        &mut self,
        term: &str,
        source: Option<String>,
    ) -> Result<DictionaryTerm, DictionaryError> {
        let (display, identity) = normalize_and_validate_term(term)?;
        let added = self.upsert_active_record(
            display,
            identity.clone(),
            source.unwrap_or_else(|| "manual-add".to_string()),
            now_utc_ms(),
        )?;
        self.store
            .queue
            .retain(|row| normalize_identity_lossy(&row.term) != identity);
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
            terms: self.get_terms(None),
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
        let original_store = self.store.clone();

        for term in &export.terms {
            let Ok((display, identity)) = normalize_and_validate_term(&term.term) else {
                skipped += 1;
                continue;
            };
            if self.contains_identity(&identity) {
                skipped += 1;
                continue;
            }
            if let Err(error) =
                self.upsert_active_record(display, identity, "import".to_string(), now_utc_ms())
            {
                self.store = original_store;
                return Err(error);
            }
            added += 1;
        }

        if added > 0 {
            if let Err(error) = self.persist() {
                self.store = original_store;
                return Err(error);
            }
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
            term_count: self.active_term_count(),
            message,
        }
    }

    pub fn get_dictionary_sync_records(&self) -> Vec<DictionarySyncRecord> {
        let mut records = self
            .store
            .terms
            .iter()
            .map(StoredDictionaryRecord::sync_record)
            .collect::<Vec<_>>();
        records.sort_by(|left, right| left.normalized_term.cmp(&right.normalized_term));
        records
    }

    pub fn reconcile_dictionary_records(
        &mut self,
        remote_records: Vec<DictionarySyncRecord>,
    ) -> Result<DictionaryReconcileResult, DictionaryError> {
        let mut remote_by_identity = HashMap::new();
        for remote in remote_records {
            let (display, identity) = normalize_and_validate_term(&remote.term)?;
            if identity != remote.normalized_term {
                return Err(DictionaryError::InvalidSyncIdentity);
            }
            let candidate = DictionarySyncRecord {
                term: display,
                normalized_term: identity.clone(),
                source: normalize_source(&remote.source),
                created_at_utc_ms: remote.created_at_utc_ms,
                updated_at_utc_ms: remote.updated_at_utc_ms,
                deleted_at_utc_ms: remote.deleted_at_utc_ms,
            };
            remote_by_identity
                .entry(identity)
                .and_modify(|current: &mut DictionarySyncRecord| {
                    if sync_record_wins(&candidate, current) {
                        *current = candidate.clone();
                    }
                })
                .or_insert(candidate);
        }

        let original_store = self.store.clone();
        let mut changed = false;
        let local_identities = self
            .store
            .terms
            .iter()
            .map(|record| record.normalized_term.clone())
            .collect::<HashSet<_>>();

        for record in &mut self.store.terms {
            let Some(remote) = remote_by_identity.remove(&record.normalized_term) else {
                continue;
            };
            if remote_wins(&remote, record) {
                record.term = remote.term;
                record.source = remote.source;
                record.created_at_utc_ms = remote.created_at_utc_ms;
                record.updated_at_utc_ms = remote.updated_at_utc_ms;
                record.deleted_at_utc_ms = remote.deleted_at_utc_ms;
                changed = true;
            }
        }

        for (identity, remote) in remote_by_identity {
            if local_identities.contains(&identity) {
                continue;
            }
            let term_id = self.next_id("dt");
            self.store.terms.push(StoredDictionaryRecord {
                term_id,
                term: remote.term,
                normalized_term: identity,
                source: remote.source,
                created_at_utc_ms: remote.created_at_utc_ms,
                updated_at_utc_ms: remote.updated_at_utc_ms,
                deleted_at_utc_ms: remote.deleted_at_utc_ms,
            });
            changed = true;
        }

        if self.active_term_count() > MAX_APPROVED_TERMS {
            self.store = original_store;
            return Err(DictionaryError::ApprovedTermLimit);
        }
        if changed {
            if let Err(error) = self.persist() {
                self.store = original_store;
                return Err(error);
            }
        }
        Ok(DictionaryReconcileResult {
            terms: self.get_terms(None),
            records: self.get_dictionary_sync_records(),
        })
    }

    fn contains_term(&self, candidate: &str) -> bool {
        normalize_and_validate_term(candidate)
            .map(|(_, identity)| self.contains_identity(&identity))
            .unwrap_or(false)
    }

    fn contains_identity(&self, identity: &str) -> bool {
        self.store
            .terms
            .iter()
            .any(|term| term.deleted_at_utc_ms.is_none() && term.normalized_term == identity)
    }

    fn in_queue(&self, candidate: &str) -> bool {
        let candidate = normalize_identity_lossy(candidate);
        self.store
            .queue
            .iter()
            .any(|item| normalize_identity_lossy(&item.term) == candidate)
    }

    fn active_term_count(&self) -> usize {
        self.store
            .terms
            .iter()
            .filter(|record| record.deleted_at_utc_ms.is_none())
            .count()
    }

    fn cap_pending_queue(&mut self) {
        if self.store.queue.len() > MAX_PENDING_TERMS {
            let overflow = self.store.queue.len() - MAX_PENDING_TERMS;
            self.store.queue.drain(0..overflow);
        }
    }

    fn upsert_active_record(
        &mut self,
        display: String,
        identity: String,
        source: String,
        requested_timestamp: u64,
    ) -> Result<DictionaryTerm, DictionaryError> {
        let active_term_count = self.active_term_count();
        if let Some(record) = self
            .store
            .terms
            .iter_mut()
            .find(|record| record.normalized_term == identity)
        {
            if record.deleted_at_utc_ms.is_none() {
                return Ok(record.public_term().expect("active record"));
            }
            if active_term_count >= MAX_APPROVED_TERMS {
                return Err(DictionaryError::ApprovedTermLimit);
            }
            let timestamp = requested_timestamp.max(record.updated_at_utc_ms.saturating_add(1));
            record.term = display;
            record.source = normalize_source(&source);
            record.updated_at_utc_ms = timestamp;
            record.deleted_at_utc_ms = None;
            let term = record.public_term().expect("resurrected record");
            return Ok(term);
        }

        if active_term_count >= MAX_APPROVED_TERMS {
            return Err(DictionaryError::ApprovedTermLimit);
        }
        let timestamp = requested_timestamp.max(1);
        let record = StoredDictionaryRecord {
            term_id: self.next_id("dt"),
            term: display,
            normalized_term: identity.clone(),
            source: normalize_source(&source),
            created_at_utc_ms: timestamp,
            updated_at_utc_ms: timestamp,
            deleted_at_utc_ms: None,
        };
        let term = record.public_term().expect("new active record");
        self.store.terms.push(record);
        Ok(term)
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
        if let Ok(encrypted) = serde_json::from_str::<EncryptedEnvelope>(&raw) {
            let plaintext = decrypt_bytes(&encrypted, &self.key, "dictionary")
                .map_err(map_secure_store_error)?;
            self.store = parse_dictionary_store(&plaintext)?;
        } else {
            self.store = parse_dictionary_store(raw.as_bytes())?;
            self.persist()?;
        }
        let before = self.store.queue.len();
        self.store
            .queue
            .retain(|item| is_high_signal_term(&item.term));
        self.cap_pending_queue();
        if self.store.queue.len() != before {
            self.persist()?;
        }
        Ok(())
    }

    fn persist(&self) -> Result<(), DictionaryError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(DictionaryError::Write)?;
        }
        let encrypted =
            encrypt_json(&self.store, &self.key, "dictionary").map_err(map_secure_store_error)?;
        let raw = serde_json::to_string_pretty(&encrypted).map_err(DictionaryError::Parse)?;
        fs::write(&self.path, raw).map_err(DictionaryError::Write)?;
        Ok(())
    }
}

fn default_next_id() -> u64 {
    1
}

fn normalize_and_validate_term(value: &str) -> Result<(String, String), DictionaryError> {
    if value.chars().any(char::is_control) {
        return Err(DictionaryError::InvalidTermCharacters);
    }
    let collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return Err(DictionaryError::EmptyTerm);
    }
    let display = collapsed.nfc().collect::<String>();
    if display.chars().count() > MAX_TERM_CHARS {
        return Err(DictionaryError::TermTooLong);
    }
    let identity = display.to_lowercase();
    Ok((display, identity))
}

fn normalize_identity_lossy(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .nfc()
        .collect::<String>()
        .to_lowercase()
}

fn normalize_source(value: &str) -> String {
    let normalized = value.trim();
    let value = if normalized.is_empty() {
        "sync"
    } else {
        normalized
    };
    value.chars().take(40).collect()
}

fn monotonic_timestamp(previous: u64) -> u64 {
    now_utc_ms().max(previous.saturating_add(1))
}

fn sync_effective_timestamp(record: &DictionarySyncRecord) -> u64 {
    record
        .deleted_at_utc_ms
        .unwrap_or(record.updated_at_utc_ms)
        .max(record.updated_at_utc_ms)
}

fn sync_record_wins(candidate: &DictionarySyncRecord, current: &DictionarySyncRecord) -> bool {
    let candidate_timestamp = sync_effective_timestamp(candidate);
    let current_timestamp = sync_effective_timestamp(current);
    candidate_timestamp > current_timestamp
        || (candidate_timestamp == current_timestamp
            && candidate.deleted_at_utc_ms.is_some()
            && current.deleted_at_utc_ms.is_none())
}

fn remote_wins(remote: &DictionarySyncRecord, local: &StoredDictionaryRecord) -> bool {
    let remote_timestamp = sync_effective_timestamp(remote);
    let local_timestamp = local.effective_timestamp();
    remote_timestamp > local_timestamp
        || (remote_timestamp == local_timestamp
            && remote.deleted_at_utc_ms.is_some()
            && local.deleted_at_utc_ms.is_none())
}

fn parse_dictionary_store(raw: &[u8]) -> Result<DictionaryStore, DictionaryError> {
    if let Ok(current) = serde_json::from_slice::<CurrentDictionaryStore>(raw) {
        return Ok(DictionaryStore {
            next_id: current.next_id.max(1),
            queue: current.queue,
            terms: current.terms,
        });
    }

    let legacy =
        serde_json::from_slice::<LegacyDictionaryStore>(raw).map_err(DictionaryError::Parse)?;
    let mut terms = Vec::with_capacity(legacy.terms.len());
    for term in legacy.terms {
        let (display, identity) = normalize_and_validate_term(&term.term)?;
        let timestamp = term.created_at_utc_ms.max(1);
        terms.push(StoredDictionaryRecord {
            term_id: term.term_id,
            term: display,
            normalized_term: identity,
            source: normalize_source(&term.source),
            created_at_utc_ms: timestamp,
            updated_at_utc_ms: timestamp,
            deleted_at_utc_ms: None,
        });
    }
    Ok(DictionaryStore {
        next_id: legacy.next_id.max(1),
        queue: legacy.queue,
        terms,
    })
}

fn map_secure_store_error(error: SecureStoreError) -> DictionaryError {
    match error {
        SecureStoreError::Read { source, .. } => DictionaryError::Read(source),
        SecureStoreError::Write { source, .. } => DictionaryError::Write(source),
        SecureStoreError::Serialize { source, .. } => DictionaryError::Parse(source),
        SecureStoreError::Encrypt { message, .. } => DictionaryError::Encrypt(message),
        SecureStoreError::Decrypt { message, .. } => DictionaryError::Decrypt(message),
        SecureStoreError::KeyDecode { message, .. } => DictionaryError::KeyDecode(message),
    }
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
    use aes_gcm::{
        aead::{Aead, KeyInit},
        Aes256Gcm, Nonce,
    };
    use base64::Engine;

    fn test_manager(name: &str) -> DictionaryManager {
        let base =
            std::env::temp_dir().join(format!("voicewave-dictionary-{name}-{}", now_utc_ms()));
        let path = base.with_extension("json");
        let key_path = base.with_extension("key");
        let key = load_or_create_key(&key_path, "dictionary").expect("key");
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
    fn pre_extraction_envelope_remains_readable() {
        let manager = test_manager("legacy-envelope");
        let path = manager.path.clone();
        let key_path = manager._key_path.clone();
        let key = manager.key;
        let store = DictionaryStore {
            next_id: 2,
            queue: Vec::new(),
            terms: vec![StoredDictionaryRecord {
                term_id: "dt-1".to_string(),
                term: "LegacyEnvelopeTerm".to_string(),
                normalized_term: "legacyenvelopeterm".to_string(),
                source: "compatibility-test".to_string(),
                created_at_utc_ms: 1_700_000_000_000,
                updated_at_utc_ms: 1_700_000_000_000,
                deleted_at_utc_ms: None,
            }],
        };

        // This deliberately reproduces the pre-extraction implementation
        // instead of using secure_store::encrypt_json to create the fixture.
        let plaintext = serde_json::to_vec(&store).expect("serialize legacy store");
        let cipher = Aes256Gcm::new_from_slice(&key).expect("legacy cipher");
        let nonce_bytes = [7_u8; 12];
        let ciphertext = cipher
            .encrypt(Nonce::from_slice(&nonce_bytes), plaintext.as_ref())
            .expect("legacy encrypt");
        let legacy_envelope = serde_json::json!({
            "version": 1,
            "nonceB64": base64::engine::general_purpose::STANDARD.encode(nonce_bytes),
            "ciphertextB64": base64::engine::general_purpose::STANDARD.encode(ciphertext),
        });
        fs::write(
            &path,
            serde_json::to_string_pretty(&legacy_envelope).expect("serialize legacy envelope"),
        )
        .expect("write legacy envelope");
        drop(manager);

        let reopened = DictionaryManager::from_paths(path, key_path).expect("open legacy envelope");
        let terms = reopened.get_terms(None);
        assert_eq!(terms.len(), 1);
        assert_eq!(terms[0].term, "LegacyEnvelopeTerm");
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

        let summary = manager
            .import_terms(&payload)
            .expect("import should succeed");
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

    fn queue_item(entry_id: &str, term: &str) -> DictionaryQueueItem {
        DictionaryQueueItem {
            entry_id: entry_id.to_string(),
            term: term.to_string(),
            source_preview: "test preview".to_string(),
            created_at_utc_ms: now_utc_ms(),
        }
    }

    fn stored_record(identity: &str, deleted: bool) -> StoredDictionaryRecord {
        StoredDictionaryRecord {
            term_id: format!("dt-{identity}"),
            term: identity.to_string(),
            normalized_term: identity.to_string(),
            source: "test".to_string(),
            created_at_utc_ms: 1_700_000_000_000,
            updated_at_utc_ms: 1_700_000_000_000,
            deleted_at_utc_ms: deleted.then_some(1_700_000_000_000),
        }
    }

    #[test]
    fn unicode_normalization_deduplicates_canonical_equivalents() {
        let mut manager = test_manager("unicode-dedupe");
        let first = manager
            .add_term("  Caf\u{e9}  Tool  ", None)
            .expect("first");
        let second = manager
            .add_term("CAFE\u{301}   TOOL", None)
            .expect("duplicate");
        assert_eq!(first.term_id, second.term_id);
        assert_eq!(manager.get_terms(None).len(), 1);
        assert_eq!(
            manager.get_dictionary_sync_records()[0].normalized_term,
            "caf\u{e9} tool"
        );
    }

    #[test]
    fn validation_rejects_empty_control_and_overlength_terms() {
        let mut manager = test_manager("validation");
        assert!(matches!(
            manager.add_term("   ", None),
            Err(DictionaryError::EmptyTerm)
        ));
        assert!(matches!(
            manager.add_term("line\nbreak", None),
            Err(DictionaryError::InvalidTermCharacters)
        ));
        assert!(matches!(
            manager.add_term("tab\tterm", None),
            Err(DictionaryError::InvalidTermCharacters)
        ));
        assert!(matches!(
            manager.add_term(&"x".repeat(MAX_TERM_CHARS + 1), None),
            Err(DictionaryError::TermTooLong)
        ));
    }

    #[test]
    fn approval_shares_validation_and_dedupe_without_losing_invalid_queue_rows() {
        let mut manager = test_manager("approval-validation");
        let existing = manager.add_term("VoiceWave", None).expect("seed");
        manager.store.queue.push(queue_item("dq-empty", "FW-V3"));
        assert!(matches!(
            manager.approve_entry("dq-empty", Some("\n".to_string())),
            Err(DictionaryError::InvalidTermCharacters)
        ));
        assert!(manager
            .store
            .queue
            .iter()
            .any(|row| row.entry_id == "dq-empty"));

        manager.store.queue.push(queue_item("dq-dupe", "voicewave"));
        let approved = manager
            .approve_entry("dq-dupe", None)
            .expect("approve duplicate");
        assert_eq!(approved.term_id, existing.term_id);
        assert_eq!(manager.get_terms(None).len(), 1);
        assert!(!manager
            .store
            .queue
            .iter()
            .any(|row| row.entry_id == "dq-dupe"));
    }

    #[test]
    fn pending_queue_caps_at_fifty_and_keeps_newest_suggestions() {
        let mut manager = test_manager("queue-cap");
        for index in 0..60 {
            manager
                .queue_correction_candidates(&[format!("TERM-{index}")], "preview")
                .expect("queue candidate");
        }
        assert_eq!(manager.store.queue.len(), MAX_PENDING_TERMS);
        assert_eq!(
            manager.store.queue.first().map(|row| row.term.as_str()),
            Some("TERM-10")
        );
        assert_eq!(
            manager.store.queue.last().map(|row| row.term.as_str()),
            Some("TERM-59")
        );
    }

    #[test]
    fn remove_creates_hidden_tombstone_and_readd_is_monotonic() {
        let mut manager = test_manager("tombstone");
        let added = manager.add_term("Tauri", None).expect("add");
        let record = manager
            .store
            .terms
            .iter_mut()
            .find(|row| row.term_id == added.term_id)
            .expect("record");
        record.updated_at_utc_ms = now_utc_ms() + 10_000;
        let before_delete = record.updated_at_utc_ms;

        manager.remove_term(&added.term_id).expect("remove");
        assert!(manager.get_terms(None).is_empty());
        assert!(manager.export_terms().terms.is_empty());
        let tombstone = manager.get_dictionary_sync_records()[0].clone();
        assert!(tombstone.deleted_at_utc_ms.is_some());
        assert!(tombstone.updated_at_utc_ms > before_delete);

        manager.add_term("TAURI", None).expect("resurrect");
        let active = manager.get_dictionary_sync_records()[0].clone();
        assert!(active.deleted_at_utc_ms.is_none());
        assert!(active.updated_at_utc_ms > tombstone.updated_at_utc_ms);
    }

    #[test]
    fn active_cap_excludes_tombstones() {
        let mut manager = test_manager("active-cap-tombstones");
        manager.store.terms = (0..MAX_APPROVED_TERMS)
            .map(|index| stored_record(&format!("deleted-{index}"), true))
            .collect();
        assert!(manager.add_term("still allowed", None).is_ok());

        manager.store.terms = (0..MAX_APPROVED_TERMS)
            .map(|index| stored_record(&format!("active-{index}"), false))
            .collect();
        assert!(matches!(
            manager.add_term("one too many", None),
            Err(DictionaryError::ApprovedTermLimit)
        ));
    }

    #[test]
    fn plaintext_store_migrates_without_plaintext_backup_residue() {
        let base = std::env::temp_dir().join(format!("voicewave-legacy-{}", now_utc_ms()));
        let path = base.with_extension("json");
        let key_path = base.with_extension("key");
        let legacy = serde_json::json!({
            "nextId": 2,
            "queue": [],
            "terms": [{
                "termId": "dt-1",
                "term": "SecretVocabulary",
                "source": "legacy",
                "createdAtUtcMs": 1_700_000_000_000_u64
            }]
        });
        fs::write(
            &path,
            serde_json::to_vec_pretty(&legacy).expect("legacy json"),
        )
        .expect("write legacy");

        let manager = DictionaryManager::from_paths(&path, &key_path).expect("migrate");
        assert_eq!(manager.get_terms(None)[0].term, "SecretVocabulary");
        let raw = fs::read_to_string(&path).expect("read encrypted");
        assert!(!raw.contains("SecretVocabulary"));
        assert!(!path.with_extension("json.bak").exists());
        let encrypted: EncryptedEnvelope = serde_json::from_str(&raw).expect("envelope");
        let plaintext = decrypt_bytes(&encrypted, &manager.key, "dictionary").expect("decrypt");
        let decrypted = parse_dictionary_store(&plaintext).expect("parse");
        assert_eq!(decrypted.terms[0].term, "SecretVocabulary");
    }

    #[test]
    fn reconciliation_applies_winner_rules_and_is_idempotent() {
        let mut manager = test_manager("reconcile");
        manager.add_term("Local Spelling", None).expect("local");
        let local = manager.get_dictionary_sync_records()[0].clone();

        let older_remote = DictionarySyncRecord {
            term: "LOCAL SPELLING".to_string(),
            normalized_term: local.normalized_term.clone(),
            source: "remote".to_string(),
            created_at_utc_ms: local.created_at_utc_ms,
            updated_at_utc_ms: local.updated_at_utc_ms.saturating_sub(1),
            deleted_at_utc_ms: None,
        };
        let tie_delete = DictionarySyncRecord {
            updated_at_utc_ms: local.updated_at_utc_ms,
            deleted_at_utc_ms: Some(local.updated_at_utc_ms),
            ..older_remote.clone()
        };
        let remote_only = DictionarySyncRecord {
            term: "Remote Only".to_string(),
            normalized_term: "remote only".to_string(),
            source: "remote".to_string(),
            created_at_utc_ms: local.created_at_utc_ms,
            updated_at_utc_ms: local.updated_at_utc_ms + 1,
            deleted_at_utc_ms: None,
        };

        let result = manager
            .reconcile_dictionary_records(vec![older_remote, tie_delete, remote_only])
            .expect("reconcile");
        assert_eq!(result.terms.len(), 1);
        assert_eq!(result.terms[0].term, "Remote Only");
        assert_eq!(result.records.len(), 2);
        assert!(result
            .records
            .iter()
            .find(|row| row.normalized_term == "local spelling")
            .and_then(|row| row.deleted_at_utc_ms)
            .is_some());

        let raw_before = fs::read_to_string(&manager.path).expect("persisted");
        let second = manager
            .reconcile_dictionary_records(result.records.clone())
            .expect("idempotent reconcile");
        assert_eq!(second, result);
        assert_eq!(
            fs::read_to_string(&manager.path).expect("persisted again"),
            raw_before
        );
    }

    #[test]
    fn reconciliation_rejects_mismatched_remote_identity() {
        let mut manager = test_manager("sync-identity");
        let remote = DictionarySyncRecord {
            term: "VoiceWave".to_string(),
            normalized_term: "wrong".to_string(),
            source: "remote".to_string(),
            created_at_utc_ms: 1_700_000_000_000,
            updated_at_utc_ms: 1_700_000_000_000,
            deleted_at_utc_ms: None,
        };
        assert!(matches!(
            manager.reconcile_dictionary_records(vec![remote]),
            Err(DictionaryError::InvalidSyncIdentity)
        ));
    }

    #[test]
    fn reconciliation_keeps_local_active_ties_and_accepts_newer_remote_changes() {
        let mut manager = test_manager("sync-active-winners");
        manager.add_term("Local Case", None).expect("local");
        let local = manager.get_dictionary_sync_records()[0].clone();
        let tied_remote = DictionarySyncRecord {
            term: "LOCAL CASE".to_string(),
            source: "remote".to_string(),
            ..local.clone()
        };
        let tied = manager
            .reconcile_dictionary_records(vec![tied_remote])
            .expect("active tie");
        assert_eq!(tied.terms[0].term, "Local Case");

        let newer_remote = DictionarySyncRecord {
            term: "LOCAL CASE".to_string(),
            source: "remote".to_string(),
            updated_at_utc_ms: local.updated_at_utc_ms + 1,
            ..local.clone()
        };
        let newer = manager
            .reconcile_dictionary_records(vec![newer_remote.clone()])
            .expect("remote newer");
        assert_eq!(newer.terms[0].term, "LOCAL CASE");

        let newer_delete = DictionarySyncRecord {
            updated_at_utc_ms: newer_remote.updated_at_utc_ms + 1,
            deleted_at_utc_ms: Some(newer_remote.updated_at_utc_ms + 1),
            ..newer_remote
        };
        let deleted = manager
            .reconcile_dictionary_records(vec![newer_delete])
            .expect("remote deletion newer");
        assert!(deleted.terms.is_empty());
    }
}
