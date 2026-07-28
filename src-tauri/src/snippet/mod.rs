use crate::atomic_file::{self, StoreLoad};
use crate::secure_store::{decrypt_bytes, encrypt_json, load_or_create_key, EncryptedEnvelope};
use base64::Engine;
use directories::ProjectDirs;
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use unicode_normalization::{char::canonical_combining_class, UnicodeNormalization};

pub const MAX_ACTIVE_SNIPPETS: usize = 250;
pub const MAX_TRIGGER_CHARS: usize = 60;
pub const MAX_EXPANSION_CHARS: usize = 4_000;
pub const MAX_EXPANSIONS_PER_DICTATION: usize = 16;

const SLOT_PREFIX: &str = "\u{e000}VW_SNIP_";
const SLOT_SUFFIX: char = '\u{e001}';
const MIN_REASONABLE_TIMESTAMP_UTC_MS: u64 = 1_609_459_200_000;
const MAX_FUTURE_CLOCK_SKEW_MS: u64 = 5 * 60 * 1_000;
const RESERVED_TRIGGERS: &[&str] = &[
    "new line",
    "next line",
    "new paragraph",
    "bullet point",
    "new bullet",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VoiceSnippet {
    pub snippet_id: String,
    pub trigger: String,
    pub normalized_trigger: String,
    pub expansion: String,
    pub created_at_utc_ms: u64,
    pub updated_at_utc_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VoiceSnippetSyncRecord {
    pub trigger: String,
    pub normalized_trigger: String,
    pub expansion: String,
    pub created_at_utc_ms: u64,
    pub updated_at_utc_ms: u64,
    pub deleted_at_utc_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VoiceSnippetReconcileResult {
    pub snippets: Vec<VoiceSnippet>,
    pub records: Vec<VoiceSnippetSyncRecord>,
    pub limit_exceeded: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum SnippetError {
    #[error("snippet trigger is empty")]
    EmptyTrigger,
    #[error("snippet trigger exceeds {MAX_TRIGGER_CHARS} characters")]
    TriggerTooLong,
    #[error("snippet trigger contains control characters")]
    InvalidTriggerCharacters,
    #[error("snippet trigger conflicts with a built-in voice command")]
    ReservedTrigger,
    #[error("snippet expansion is empty")]
    EmptyExpansion,
    #[error("snippet expansion exceeds {MAX_EXPANSION_CHARS} characters")]
    ExpansionTooLong,
    #[error("snippet expansion contains unsafe control characters")]
    InvalidExpansionCharacters,
    #[error("a snippet already uses this trigger")]
    DuplicateTrigger,
    #[error("snippet not found: {0}")]
    NotFound(String),
    #[error("snippet active limit of {MAX_ACTIVE_SNIPPETS} reached")]
    ActiveLimit,
    #[error("snippet sync record identity does not match its normalized trigger")]
    InvalidSyncIdentity,
    #[error("snippet sync record timestamps are invalid")]
    InvalidSyncTimestamps,
    #[error("snippet tombstone must not retain expansion content")]
    TombstoneContainsExpansion,
    #[error("cannot resolve app data directory")]
    AppData,
    #[error("failed to persist snippets: {0}")]
    Persistence(String),
    #[error("failed to parse snippet store: {0}")]
    Parse(serde_json::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct StoredSnippetRecord {
    snippet_id: String,
    trigger: String,
    normalized_trigger: String,
    expansion: String,
    created_at_utc_ms: u64,
    updated_at_utc_ms: u64,
    deleted_at_utc_ms: Option<u64>,
}

impl StoredSnippetRecord {
    fn public(&self) -> Option<VoiceSnippet> {
        self.deleted_at_utc_ms.is_none().then(|| VoiceSnippet {
            snippet_id: self.snippet_id.clone(),
            trigger: self.trigger.clone(),
            normalized_trigger: self.normalized_trigger.clone(),
            expansion: self.expansion.clone(),
            created_at_utc_ms: self.created_at_utc_ms,
            updated_at_utc_ms: self.updated_at_utc_ms,
        })
    }

    fn sync_record(&self) -> VoiceSnippetSyncRecord {
        VoiceSnippetSyncRecord {
            trigger: self.trigger.clone(),
            normalized_trigger: self.normalized_trigger.clone(),
            expansion: self.expansion.clone(),
            created_at_utc_ms: self.created_at_utc_ms,
            updated_at_utc_ms: self.updated_at_utc_ms,
            deleted_at_utc_ms: self.deleted_at_utc_ms,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SnippetStore {
    #[serde(default = "default_next_id")]
    next_id: u64,
    #[serde(default)]
    snippets: Vec<StoredSnippetRecord>,
}

impl Default for SnippetStore {
    fn default() -> Self {
        Self {
            next_id: 1,
            snippets: Vec::new(),
        }
    }
}

pub struct SnippetManager {
    path: PathBuf,
    _key_path: PathBuf,
    key: [u8; 32],
    store: SnippetStore,
}

impl SnippetManager {
    pub fn new() -> Result<Self, SnippetError> {
        let project =
            ProjectDirs::from("com", "voicewave", "localcore").ok_or(SnippetError::AppData)?;
        Self::from_paths(
            project.config_dir().join("snippets.json"),
            project.config_dir().join("snippets.key"),
        )
    }

    pub fn from_paths(
        path: impl AsRef<Path>,
        key_path: impl AsRef<Path>,
    ) -> Result<Self, SnippetError> {
        let path = path.as_ref().to_path_buf();
        let key_path = key_path.as_ref().to_path_buf();
        atomic_file::recover_interrupted_replace(&path)
            .map_err(|error| SnippetError::Persistence(error.to_string()))?;
        let key = load_or_create_key(&key_path, "snippets")
            .map_err(|error| SnippetError::Persistence(error.to_string()))?;
        let mut manager = Self {
            path,
            _key_path: key_path,
            key,
            store: SnippetStore::default(),
        };
        manager.load()?;
        Ok(manager)
    }

    pub fn list_snippets(&self, query: Option<String>) -> Vec<VoiceSnippet> {
        let query = query.unwrap_or_default();
        let trigger_query = normalize_identity_lossy(&query);
        let expansion_query = query.nfc().collect::<String>().to_lowercase();
        let mut rows = self
            .store
            .snippets
            .iter()
            .filter(|record| {
                record.deleted_at_utc_ms.is_none()
                    && (query.trim().is_empty()
                        || record.normalized_trigger.contains(&trigger_query)
                        || record
                            .expansion
                            .nfc()
                            .collect::<String>()
                            .to_lowercase()
                            .contains(&expansion_query))
            })
            .filter_map(StoredSnippetRecord::public)
            .collect::<Vec<_>>();
        rows.sort_by(|left, right| {
            left.normalized_trigger
                .cmp(&right.normalized_trigger)
                .then_with(|| left.snippet_id.cmp(&right.snippet_id))
        });
        rows
    }

    pub fn add_snippet(
        &mut self,
        trigger: &str,
        expansion: &str,
    ) -> Result<VoiceSnippet, SnippetError> {
        let (trigger, identity) = normalize_and_validate_trigger(trigger)?;
        let expansion = normalize_and_validate_expansion(expansion)?;
        if self.active_count() >= MAX_ACTIVE_SNIPPETS {
            return Err(SnippetError::ActiveLimit);
        }
        if self.store.snippets.iter().any(|record| {
            record.normalized_trigger == identity && record.deleted_at_utc_ms.is_none()
        }) {
            return Err(SnippetError::DuplicateTrigger);
        }

        let previous = self.store.clone();
        let timestamp = monotonic_timestamp(
            self.store
                .snippets
                .iter()
                .filter(|record| record.normalized_trigger == identity)
                .map(effective_stored_timestamp)
                .max()
                .unwrap_or_default(),
        );
        let snippet = if let Some(record) = self
            .store
            .snippets
            .iter_mut()
            .find(|record| record.normalized_trigger == identity)
        {
            record.trigger = trigger;
            record.expansion = expansion;
            record.updated_at_utc_ms = timestamp;
            record.deleted_at_utc_ms = None;
            record.public().expect("resurrected snippet")
        } else {
            let record = StoredSnippetRecord {
                snippet_id: self.next_id(),
                trigger,
                normalized_trigger: identity,
                expansion,
                created_at_utc_ms: timestamp,
                updated_at_utc_ms: timestamp,
                deleted_at_utc_ms: None,
            };
            let snippet = record.public().expect("active snippet");
            self.store.snippets.push(record);
            snippet
        };
        self.persist_or_restore(previous)?;
        Ok(snippet)
    }

    pub fn update_snippet(
        &mut self,
        snippet_id: &str,
        trigger: &str,
        expansion: &str,
    ) -> Result<VoiceSnippet, SnippetError> {
        let (trigger, identity) = normalize_and_validate_trigger(trigger)?;
        let expansion = normalize_and_validate_expansion(expansion)?;
        let source_index = self
            .store
            .snippets
            .iter()
            .position(|record| {
                record.snippet_id == snippet_id && record.deleted_at_utc_ms.is_none()
            })
            .ok_or_else(|| SnippetError::NotFound(snippet_id.to_string()))?;
        let old_identity = self.store.snippets[source_index].normalized_trigger.clone();
        if identity != old_identity
            && self.store.snippets.iter().any(|record| {
                record.normalized_trigger == identity && record.deleted_at_utc_ms.is_none()
            })
        {
            return Err(SnippetError::DuplicateTrigger);
        }

        let previous = self.store.clone();
        if identity == old_identity {
            let record = &mut self.store.snippets[source_index];
            record.trigger = trigger;
            record.expansion = expansion;
            record.updated_at_utc_ms = monotonic_timestamp(record.updated_at_utc_ms);
            let snippet = record.public().expect("updated active snippet");
            self.persist_or_restore(previous)?;
            return Ok(snippet);
        }

        let target_timestamp = self
            .store
            .snippets
            .iter()
            .filter(|record| record.normalized_trigger == identity)
            .map(effective_stored_timestamp)
            .max()
            .unwrap_or_default();
        let timestamp = monotonic_timestamp(
            effective_stored_timestamp(&self.store.snippets[source_index]).max(target_timestamp),
        );
        {
            let source = &mut self.store.snippets[source_index];
            source.expansion.clear();
            source.updated_at_utc_ms = timestamp;
            source.deleted_at_utc_ms = Some(timestamp);
        }
        let snippet = if let Some(target) = self
            .store
            .snippets
            .iter_mut()
            .find(|record| record.normalized_trigger == identity)
        {
            target.trigger = trigger;
            target.expansion = expansion;
            target.updated_at_utc_ms = timestamp;
            target.deleted_at_utc_ms = None;
            target.public().expect("renamed active snippet")
        } else {
            let target = StoredSnippetRecord {
                snippet_id: self.next_id(),
                trigger,
                normalized_trigger: identity,
                expansion,
                created_at_utc_ms: timestamp,
                updated_at_utc_ms: timestamp,
                deleted_at_utc_ms: None,
            };
            let snippet = target.public().expect("renamed active snippet");
            self.store.snippets.push(target);
            snippet
        };
        self.persist_or_restore(previous)?;
        Ok(snippet)
    }

    pub fn remove_snippet(&mut self, snippet_id: &str) -> Result<(), SnippetError> {
        let index = self
            .store
            .snippets
            .iter()
            .position(|record| {
                record.snippet_id == snippet_id && record.deleted_at_utc_ms.is_none()
            })
            .ok_or_else(|| SnippetError::NotFound(snippet_id.to_string()))?;
        let previous = self.store.clone();
        let record = &mut self.store.snippets[index];
        let timestamp = monotonic_timestamp(record.updated_at_utc_ms);
        record.expansion.clear();
        record.updated_at_utc_ms = timestamp;
        record.deleted_at_utc_ms = Some(timestamp);
        self.persist_or_restore(previous)
    }

    pub fn get_sync_records(&self) -> Vec<VoiceSnippetSyncRecord> {
        let mut records = self
            .store
            .snippets
            .iter()
            .map(StoredSnippetRecord::sync_record)
            .collect::<Vec<_>>();
        records.sort_by(|left, right| left.normalized_trigger.cmp(&right.normalized_trigger));
        records
    }

    pub fn reconcile_records(
        &mut self,
        remote_records: Vec<VoiceSnippetSyncRecord>,
    ) -> Result<VoiceSnippetReconcileResult, SnippetError> {
        let mut remote_by_identity = HashMap::new();
        for record in remote_records {
            let canonical = validate_sync_record(record)?;
            remote_by_identity
                .entry(canonical.normalized_trigger.clone())
                .and_modify(|current: &mut VoiceSnippetSyncRecord| {
                    if sync_record_wins(&canonical, current) {
                        *current = canonical.clone();
                    }
                })
                .or_insert(canonical);
        }

        let previous = self.store.clone();
        for (identity, remote) in remote_by_identity {
            if let Some(local) = self
                .store
                .snippets
                .iter_mut()
                .find(|record| record.normalized_trigger == identity)
            {
                if sync_record_wins(&remote, &local.sync_record()) {
                    local.trigger = remote.trigger;
                    local.expansion = remote.expansion;
                    local.created_at_utc_ms = remote.created_at_utc_ms;
                    local.updated_at_utc_ms = remote.updated_at_utc_ms;
                    local.deleted_at_utc_ms = remote.deleted_at_utc_ms;
                }
            } else {
                let snippet_id = self.next_id();
                self.store.snippets.push(StoredSnippetRecord {
                    snippet_id,
                    trigger: remote.trigger,
                    normalized_trigger: remote.normalized_trigger,
                    expansion: remote.expansion,
                    created_at_utc_ms: remote.created_at_utc_ms,
                    updated_at_utc_ms: remote.updated_at_utc_ms,
                    deleted_at_utc_ms: remote.deleted_at_utc_ms,
                });
            }
        }
        self.persist_or_restore(previous)?;
        Ok(self.reconcile_result())
    }

    fn reconcile_result(&self) -> VoiceSnippetReconcileResult {
        VoiceSnippetReconcileResult {
            snippets: self.list_snippets(None),
            records: self.get_sync_records(),
            limit_exceeded: self.active_count() > MAX_ACTIVE_SNIPPETS,
        }
    }

    fn active_count(&self) -> usize {
        self.store
            .snippets
            .iter()
            .filter(|record| record.deleted_at_utc_ms.is_none())
            .count()
    }

    fn next_id(&mut self) -> String {
        let id = self.store.next_id;
        self.store.next_id += 1;
        format!("vs-{id}")
    }

    /// Loads snippets, resetting rather than erroring when neither the primary
    /// nor its backup decodes — a broken store must not stop the app booting.
    fn load(&mut self) -> Result<(), SnippetError> {
        let key = self.key;
        let outcome =
            atomic_file::load_with_recovery(&self.path, "snippets", |raw| decode_store(raw, &key));
        match outcome {
            StoreLoad::Missing => Ok(()),
            StoreLoad::Loaded(store) | StoreLoad::Recovered(store) => {
                self.store = store;
                Ok(())
            }
            StoreLoad::Reset => {
                self.store = SnippetStore::default();
                let _ = self.persist();
                Ok(())
            }
        }
    }

    fn persist_or_restore(&mut self, previous: SnippetStore) -> Result<(), SnippetError> {
        if let Err(error) = self.persist() {
            self.store = previous;
            return Err(error);
        }
        Ok(())
    }

    fn persist(&self) -> Result<(), SnippetError> {
        let envelope = encrypt_json(&self.store, &self.key, "snippets")
            .map_err(|error| SnippetError::Persistence(error.to_string()))?;
        let raw = serde_json::to_vec_pretty(&envelope).map_err(SnippetError::Parse)?;
        atomic_file::atomic_write(&self.path, &raw)
            .map_err(|error| SnippetError::Persistence(error.to_string()))
    }
}

#[derive(Debug, Clone)]
struct NormalizedChar {
    value: char,
    source_start: usize,
    source_end: usize,
}

#[derive(Debug)]
struct NormalizedView {
    chars: Vec<NormalizedChar>,
}

impl NormalizedView {
    fn from_source(source: &str) -> Self {
        let mut chars = Vec::new();
        let mut token_start = None;
        let mut whitespace_start = None;
        let indexed = source
            .char_indices()
            .map(|(start, value)| (start, start + value.len_utf8(), value))
            .collect::<Vec<_>>();

        for (index, &(start, end, value)) in indexed.iter().enumerate() {
            if value.is_whitespace() {
                if let Some(token_start) = token_start.take() {
                    append_normalized_token(source, &indexed[token_start..index], &mut chars);
                }
                whitespace_start.get_or_insert(start);
            } else {
                if let Some(space_start) = whitespace_start.take() {
                    if !chars.is_empty() {
                        chars.push(NormalizedChar {
                            value: ' ',
                            source_start: space_start,
                            source_end: start,
                        });
                    }
                }
                token_start.get_or_insert(index);
            }
            if index + 1 == indexed.len() {
                if let Some(token_start) = token_start.take() {
                    append_normalized_token(source, &indexed[token_start..], &mut chars);
                } else if let Some(space_start) = whitespace_start.take() {
                    if let Some(last) = chars.last_mut().filter(|last| last.value == ' ') {
                        last.source_start = space_start;
                        last.source_end = end;
                    }
                }
            }
        }
        while chars.last().is_some_and(|value| value.value == ' ') {
            chars.pop();
        }
        Self { chars }
    }

    fn values(&self) -> Vec<char> {
        self.chars.iter().map(|value| value.value).collect()
    }
}

fn append_normalized_token(
    source: &str,
    token: &[(usize, usize, char)],
    output: &mut Vec<NormalizedChar>,
) {
    let Some((token_start, _, _)) = token.first().copied() else {
        return;
    };
    let mut previous = Vec::<NormalizedChar>::new();
    for &(_, end, _) in token {
        let normalized = source[token_start..end]
            .nfc()
            .collect::<String>()
            .to_lowercase()
            .chars()
            .collect::<Vec<_>>();
        let common = previous
            .iter()
            .map(|value| value.value)
            .zip(normalized.iter().copied())
            .take_while(|(left, right)| left == right)
            .count();
        let changed_start = previous
            .get(common)
            .map(|value| value.source_start)
            .unwrap_or_else(|| {
                token
                    .get(common)
                    .map(|value| value.0)
                    .unwrap_or(token_start)
            });
        previous.truncate(common);
        previous.extend(
            normalized[common..]
                .iter()
                .copied()
                .map(|value| NormalizedChar {
                    value,
                    source_start: changed_start,
                    source_end: end,
                }),
        );
    }
    output.extend(previous);
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SelectedMatch {
    source_start: usize,
    source_end: usize,
    normalized_len: usize,
    identity: String,
    expansion: String,
}

#[derive(Debug, Clone)]
pub struct SnippetExpansionPlan {
    source_text: String,
    protected_text: String,
    slots: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub enum ProtectionOutcome {
    NoMatch,
    ExactOnly { expansion: String },
    Inline(SnippetExpansionPlan),
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum ProtectionError {
    #[error("too many snippet expansions in one dictation")]
    TooManyMatches,
    #[error("a protected snippet slot is missing, duplicated, or changed")]
    InvalidSlots,
}

impl SnippetExpansionPlan {
    pub fn protect(
        source: &str,
        snippets: &[VoiceSnippet],
    ) -> Result<ProtectionOutcome, ProtectionError> {
        if let Some(expansion) = exact_only_expansion(source, snippets) {
            return Ok(ProtectionOutcome::ExactOnly { expansion });
        }
        let matches = select_matches(source, snippets);
        if matches.is_empty() {
            return Ok(ProtectionOutcome::NoMatch);
        }
        if matches.len() > MAX_EXPANSIONS_PER_DICTATION {
            return Err(ProtectionError::TooManyMatches);
        }

        let mut nonce = [0_u8; 16];
        OsRng.fill_bytes(&mut nonce);
        let nonce = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(nonce);
        let mut protected = String::with_capacity(source.len());
        let mut slots = Vec::with_capacity(matches.len());
        let mut cursor = 0;
        for (index, selected) in matches.into_iter().enumerate() {
            protected.push_str(&source[cursor..selected.source_start]);
            let slot = format!("{SLOT_PREFIX}{nonce}_{index}{SLOT_SUFFIX}");
            protected.push_str(&slot);
            slots.push((slot, selected.expansion));
            cursor = selected.source_end;
        }
        protected.push_str(&source[cursor..]);
        Ok(ProtectionOutcome::Inline(Self {
            source_text: source.to_string(),
            protected_text: protected,
            slots,
        }))
    }

    pub fn protected_text(&self) -> &str {
        &self.protected_text
    }

    pub fn source_text(&self) -> &str {
        &self.source_text
    }

    pub fn match_count(&self) -> usize {
        self.slots.len()
    }

    pub fn validate_candidate(&self, candidate: &str) -> Result<(), ProtectionError> {
        if self
            .slots
            .iter()
            .any(|(slot, _)| candidate.match_indices(slot).count() != 1)
        {
            return Err(ProtectionError::InvalidSlots);
        }
        let mut without_expected = candidate.to_string();
        for (slot, _) in &self.slots {
            without_expected = without_expected.replacen(slot, "", 1);
        }
        if without_expected.contains(SLOT_PREFIX)
            || without_expected.contains('\u{e000}')
            || without_expected.contains(SLOT_SUFFIX)
        {
            return Err(ProtectionError::InvalidSlots);
        }
        Ok(())
    }

    pub fn restore(&self, candidate: &str) -> Result<String, ProtectionError> {
        self.validate_candidate(candidate)?;
        let mut restored = candidate.to_string();
        for (slot, expansion) in &self.slots {
            restored = restored.replacen(slot, expansion, 1);
        }
        Ok(restored)
    }
}

fn select_matches(source: &str, snippets: &[VoiceSnippet]) -> Vec<SelectedMatch> {
    let view = NormalizedView::from_source(source);
    let source_chars = view.values();
    let mut candidates = Vec::new();
    for snippet in snippets {
        let trigger = snippet.normalized_trigger.chars().collect::<Vec<_>>();
        if trigger.is_empty() || trigger.len() > source_chars.len() {
            continue;
        }
        for start in 0..=source_chars.len() - trigger.len() {
            let end = start + trigger.len();
            if source_chars[start..end] != trigger[..]
                || start
                    .checked_sub(1)
                    .and_then(|index| source_chars.get(index))
                    .is_some_and(|value| is_word_constituent(*value))
                || source_chars
                    .get(end)
                    .is_some_and(|value| is_word_constituent(*value))
            {
                continue;
            }
            candidates.push(SelectedMatch {
                source_start: view.chars[start].source_start,
                source_end: view.chars[end - 1].source_end,
                normalized_len: trigger.len(),
                identity: snippet.normalized_trigger.clone(),
                expansion: snippet.expansion.clone(),
            });
        }
    }
    candidates.sort_by(|left, right| {
        right
            .normalized_len
            .cmp(&left.normalized_len)
            .then_with(|| left.source_start.cmp(&right.source_start))
            .then_with(|| left.identity.cmp(&right.identity))
    });
    let mut selected = Vec::<SelectedMatch>::new();
    for candidate in candidates {
        if selected.iter().all(|chosen| {
            candidate.source_end <= chosen.source_start
                || candidate.source_start >= chosen.source_end
        }) {
            selected.push(candidate);
        }
    }
    selected.sort_by_key(|value| value.source_start);
    selected
}

fn exact_only_expansion(source: &str, snippets: &[VoiceSnippet]) -> Option<String> {
    let mut candidate = source.trim();
    let full_identity = normalize_identity_lossy(candidate);
    if let Some(snippet) = snippets
        .iter()
        .find(|snippet| snippet.normalized_trigger == full_identity)
    {
        return Some(snippet.expansion.clone());
    }
    if let Some(last) = candidate
        .chars()
        .last()
        .filter(|value| matches!(value, '.' | '!' | '?'))
    {
        candidate = candidate[..candidate.len() - last.len_utf8()].trim_end();
    }
    let identity = normalize_identity_lossy(candidate);
    snippets
        .iter()
        .find(|snippet| snippet.normalized_trigger == identity)
        .map(|snippet| snippet.expansion.clone())
}

fn is_word_constituent(value: char) -> bool {
    value.is_alphanumeric() || canonical_combining_class(value) != 0
}

pub fn normalize_trigger_identity(value: &str) -> Result<String, SnippetError> {
    normalize_and_validate_trigger(value).map(|(_, identity)| identity)
}

fn normalize_and_validate_trigger(value: &str) -> Result<(String, String), SnippetError> {
    if value.chars().any(char::is_control) {
        return Err(SnippetError::InvalidTriggerCharacters);
    }
    let display = value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .nfc()
        .collect::<String>();
    if display.is_empty() {
        return Err(SnippetError::EmptyTrigger);
    }
    if display.chars().count() > MAX_TRIGGER_CHARS {
        return Err(SnippetError::TriggerTooLong);
    }
    let identity = display.to_lowercase();
    if RESERVED_TRIGGERS.contains(&identity.as_str()) {
        return Err(SnippetError::ReservedTrigger);
    }
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

fn normalize_and_validate_expansion(value: &str) -> Result<String, SnippetError> {
    let normalized = value.replace("\r\n", "\n");
    if normalized.chars().any(|value| {
        (value.is_control() && value != '\n' && value != '\t')
            || matches!(value, '\u{e000}' | '\u{e001}')
    }) {
        return Err(SnippetError::InvalidExpansionCharacters);
    }
    if normalized.chars().all(char::is_whitespace) {
        return Err(SnippetError::EmptyExpansion);
    }
    if normalized.chars().count() > MAX_EXPANSION_CHARS {
        return Err(SnippetError::ExpansionTooLong);
    }
    Ok(normalized)
}

fn validate_sync_record(
    record: VoiceSnippetSyncRecord,
) -> Result<VoiceSnippetSyncRecord, SnippetError> {
    validate_record(record, true)
}

fn validate_stored_record(
    record: VoiceSnippetSyncRecord,
) -> Result<VoiceSnippetSyncRecord, SnippetError> {
    validate_record(record, false)
}

fn validate_record(
    mut record: VoiceSnippetSyncRecord,
    enforce_wall_clock_bounds: bool,
) -> Result<VoiceSnippetSyncRecord, SnippetError> {
    let (trigger, identity) = normalize_and_validate_trigger(&record.trigger)?;
    if identity != record.normalized_trigger {
        return Err(SnippetError::InvalidSyncIdentity);
    }
    let latest_reasonable_timestamp = now_utc_ms().saturating_add(MAX_FUTURE_CLOCK_SKEW_MS);
    if (enforce_wall_clock_bounds
        && (record.created_at_utc_ms < MIN_REASONABLE_TIMESTAMP_UTC_MS
            || record.created_at_utc_ms > latest_reasonable_timestamp
            || record.updated_at_utc_ms > latest_reasonable_timestamp))
        || record.updated_at_utc_ms < record.created_at_utc_ms
        || record.deleted_at_utc_ms.is_some_and(|deleted| {
            deleted < record.updated_at_utc_ms
                || (enforce_wall_clock_bounds && deleted > latest_reasonable_timestamp)
        })
    {
        return Err(SnippetError::InvalidSyncTimestamps);
    }
    if record.deleted_at_utc_ms.is_some() {
        if !record.expansion.is_empty() {
            return Err(SnippetError::TombstoneContainsExpansion);
        }
    } else {
        record.expansion = normalize_and_validate_expansion(&record.expansion)?;
    }
    record.trigger = trigger;
    record.normalized_trigger = identity;
    Ok(record)
}

/// The store file is AES-GCM authenticated, so an invalid stored record can
/// only come from a different app version (schema drift, a grown reserved
/// list), never tampering. Dropping such records keeps every guarantee the
/// old fail-hard validation gave without bricking app startup over them.
fn sanitize_stored_records(mut store: SnippetStore) -> SnippetStore {
    let mut dropped = 0_usize;
    let mut order = Vec::new();
    let mut winners: HashMap<String, StoredSnippetRecord> = HashMap::new();
    for record in store.snippets.drain(..) {
        if validate_stored_record(record.sync_record()).is_err() {
            dropped += 1;
            continue;
        }
        match winners.entry(record.normalized_trigger.clone()) {
            std::collections::hash_map::Entry::Occupied(mut current) => {
                dropped += 1;
                if sync_record_wins(&record.sync_record(), &current.get().sync_record()) {
                    current.insert(record);
                }
            }
            std::collections::hash_map::Entry::Vacant(slot) => {
                order.push(record.normalized_trigger.clone());
                slot.insert(record);
            }
        }
    }
    if dropped > 0 {
        // Count only — never trigger or expansion content.
        eprintln!("voicewave: dropped {dropped} invalid stored snippet record(s) at load");
    }
    store.snippets = order
        .into_iter()
        .filter_map(|identity| winners.remove(&identity))
        .collect();
    store
}

fn sync_record_wins(candidate: &VoiceSnippetSyncRecord, current: &VoiceSnippetSyncRecord) -> bool {
    let candidate_timestamp = effective_sync_timestamp(candidate);
    let current_timestamp = effective_sync_timestamp(current);
    candidate_timestamp > current_timestamp
        || (candidate_timestamp == current_timestamp
            && ((candidate.deleted_at_utc_ms.is_some() && current.deleted_at_utc_ms.is_none())
                || (candidate.deleted_at_utc_ms.is_some() == current.deleted_at_utc_ms.is_some()
                    && canonical_record_tuple(candidate) > canonical_record_tuple(current))))
}

fn canonical_record_tuple(record: &VoiceSnippetSyncRecord) -> (&str, &str, u64, u64, Option<u64>) {
    (
        &record.trigger,
        &record.expansion,
        record.created_at_utc_ms,
        record.updated_at_utc_ms,
        record.deleted_at_utc_ms,
    )
}

fn effective_sync_timestamp(record: &VoiceSnippetSyncRecord) -> u64 {
    record
        .deleted_at_utc_ms
        .unwrap_or(record.updated_at_utc_ms)
        .max(record.updated_at_utc_ms)
}

fn effective_stored_timestamp(record: &StoredSnippetRecord) -> u64 {
    record
        .deleted_at_utc_ms
        .unwrap_or(record.updated_at_utc_ms)
        .max(record.updated_at_utc_ms)
}

fn monotonic_timestamp(previous: u64) -> u64 {
    now_utc_ms().max(previous.saturating_add(1))
}

fn default_next_id() -> u64 {
    1
}

fn now_utc_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

/// Decodes an encrypted snippet payload. Shared by the primary and the `.bak`
/// attempt inside [`atomic_file::load_with_recovery`].
fn decode_store(raw: &str, key: &[u8; 32]) -> Result<SnippetStore, SnippetError> {
    let envelope = serde_json::from_str::<EncryptedEnvelope>(raw).map_err(SnippetError::Parse)?;
    let plaintext = decrypt_bytes(&envelope, key, "snippets")
        .map_err(|error| SnippetError::Persistence(error.to_string()))?;
    let store = serde_json::from_slice::<SnippetStore>(&plaintext).map_err(SnippetError::Parse)?;
    Ok(sanitize_stored_records(store))
}

#[cfg(test)]
mod manager_tests {
    use super::*;

    fn manager(name: &str) -> SnippetManager {
        let base = std::env::temp_dir().join(format!("voicewave-snippet-{name}-{}", now_utc_ms()));
        SnippetManager::from_paths(base.with_extension("json"), base.with_extension("key"))
            .expect("manager")
    }

    fn sync_record(trigger: &str, expansion: &str, timestamp: u64) -> VoiceSnippetSyncRecord {
        VoiceSnippetSyncRecord {
            trigger: trigger.to_string(),
            normalized_trigger: normalize_trigger_identity(trigger).expect("identity"),
            expansion: expansion.to_string(),
            created_at_utc_ms: timestamp,
            updated_at_utc_ms: timestamp,
            deleted_at_utc_ms: None,
        }
    }

    /// Snippets already recovered from a `.bak`; this covers the case where
    /// there is no usable copy at all and the constructor must still succeed.
    #[test]
    fn corrupt_store_loads_defaults_instead_of_failing() {
        for (case, payload, expect_quarantine) in [
            ("nul", vec![0_u8; 299], false),
            (
                "truncated",
                br#"{"version":1,"nonceB64":"AAAA"#.to_vec(),
                true,
            ),
            ("garbage", vec![0x8f, 0x2c, 0xff, 0x00, 0x41, 0xfe], true),
        ] {
            let dir = std::env::temp_dir()
                .join(format!("voicewave-snippet-corrupt-{case}-{}", now_utc_ms()));
            std::fs::create_dir_all(&dir).expect("case dir");
            let path = dir.join("snippets.json");
            std::fs::write(&path, &payload).expect("seed corrupt store");

            let manager = SnippetManager::from_paths(&path, dir.join("snippets.key"))
                .expect("a corrupt snippet store must not stop the app");
            assert!(manager.list_snippets(None).is_empty(), "{case}");

            let quarantined = std::fs::read_dir(&dir)
                .expect("read case dir")
                .flatten()
                .filter(|entry| entry.file_name().to_string_lossy().contains(".corrupt-"))
                .count();
            assert_eq!(quarantined, usize::from(expect_quarantine), "{case}");

            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn manager_encrypted_reload_and_literal_validation() {
        let mut store_manager = manager("encrypted-reload");
        let path = store_manager.path.clone();
        let key_path = store_manager._key_path.clone();
        let saved = store_manager
            .add_snippet("  My   Reply  ", "Line one\r\n\tLine TWO")
            .expect("add");
        assert_eq!(saved.normalized_trigger, "my reply");
        assert_eq!(saved.expansion, "Line one\n\tLine TWO");
        let raw = std::fs::read_to_string(&path).expect("encrypted file");
        assert!(!raw.contains("Line TWO"));
        assert!(raw.contains("ciphertextB64"));
        drop(store_manager);
        let reopened = SnippetManager::from_paths(path, key_path).expect("reload");
        assert_eq!(reopened.list_snippets(None)[0], saved);

        let mut invalid = manager("validation");
        assert!(matches!(
            invalid.add_snippet("bad\ntrigger", "value"),
            Err(SnippetError::InvalidTriggerCharacters)
        ));
        assert!(matches!(
            invalid.add_snippet("okay", "\n\t"),
            Err(SnippetError::EmptyExpansion)
        ));
        assert!(matches!(
            invalid.add_snippet("okay", "bad\rvalue"),
            Err(SnippetError::InvalidExpansionCharacters)
        ));
        assert!(matches!(
            invalid.add_snippet("okay", "reserved \u{e000} slot"),
            Err(SnippetError::InvalidExpansionCharacters)
        ));
    }

    #[test]
    fn manager_duplicate_edit_atomic_rename_delete_and_readd() {
        let mut manager = manager("crud");
        let first = manager.add_snippet("Caf\u{e9}", "one").expect("first");
        assert!(matches!(
            manager.add_snippet("CAFE\u{301}", "two"),
            Err(SnippetError::DuplicateTrigger)
        ));
        let edited = manager
            .update_snippet(&first.snippet_id, "CAF\u{c9}", "edited")
            .expect("same identity edit");
        assert_eq!(edited.snippet_id, first.snippet_id);

        manager.add_snippet("occupied", "other").expect("occupied");
        assert!(matches!(
            manager.update_snippet(&first.snippet_id, "OCCUPIED", "collision"),
            Err(SnippetError::DuplicateTrigger)
        ));
        let renamed = manager
            .update_snippet(&first.snippet_id, "new identity", "renamed")
            .expect("rename");
        let records = manager.get_sync_records();
        let old = records
            .iter()
            .find(|record| record.normalized_trigger == "caf\u{e9}")
            .expect("old tombstone");
        assert!(old.deleted_at_utc_ms.is_some());
        assert!(old.expansion.is_empty());
        manager.remove_snippet(&renamed.snippet_id).expect("delete");
        assert!(manager
            .get_sync_records()
            .iter()
            .find(|record| record.normalized_trigger == "new identity")
            .expect("tombstone")
            .expansion
            .is_empty());
        let readded = manager
            .add_snippet("NEW IDENTITY", "resurrected")
            .expect("readd");
        assert_eq!(readded.snippet_id, renamed.snippet_id);
    }

    #[test]
    fn manager_reconcile_is_deterministic_and_tombstones_win_ties() {
        let timestamp = 1_700_000_000_000;
        let left = sync_record("Tie", "aaa", timestamp);
        let right = sync_record("TIE", "zzz", timestamp);
        let mut first = manager("tie-first");
        let mut second = manager("tie-second");
        let result_one = first
            .reconcile_records(vec![left.clone(), right.clone()])
            .expect("first order");
        let result_two = second
            .reconcile_records(vec![right, left])
            .expect("second order");
        assert_eq!(result_one.records, result_two.records);

        let active = result_one.records[0].clone();
        let tombstone = VoiceSnippetSyncRecord {
            expansion: String::new(),
            deleted_at_utc_ms: Some(active.updated_at_utc_ms),
            ..active
        };
        assert!(first
            .reconcile_records(vec![tombstone])
            .expect("tie delete")
            .snippets
            .is_empty());
    }

    #[test]
    fn manager_reconcile_preserves_newer_local_tombstone_and_accepts_newer_remote() {
        let mut manager = manager("stale-remote");
        let local = manager.add_snippet("local", "secret").expect("add");
        manager.remove_snippet(&local.snippet_id).expect("remove");
        let tombstone = manager.get_sync_records()[0].clone();
        let stale = sync_record(
            "local",
            "stale",
            tombstone.updated_at_utc_ms.saturating_sub(1),
        );
        assert!(manager
            .reconcile_records(vec![stale])
            .expect("stale ignored")
            .snippets
            .is_empty());
        let newer = sync_record("local", "newer", tombstone.updated_at_utc_ms + 1);
        assert_eq!(
            manager
                .reconcile_records(vec![newer])
                .expect("newer accepted")
                .snippets[0]
                .expansion,
            "newer"
        );
    }

    #[test]
    fn manager_reconcile_over_limit_is_recoverable() {
        let mut manager = manager("over-limit");
        let records = (0..=MAX_ACTIVE_SNIPPETS)
            .map(|index| {
                sync_record(
                    &format!("trigger {index}"),
                    "value",
                    1_700_000_000_000 + index as u64,
                )
            })
            .collect();
        let result = manager
            .reconcile_records(records)
            .expect("import all winners");
        assert!(result.limit_exceeded);
        assert_eq!(result.snippets.len(), MAX_ACTIVE_SNIPPETS + 1);
        assert!(matches!(
            manager.add_snippet("blocked", "value"),
            Err(SnippetError::ActiveLimit)
        ));
        let first = result.snippets[0].clone();
        assert!(manager
            .update_snippet(&first.snippet_id, &first.trigger, "edited")
            .is_ok());
        manager
            .remove_snippet(&first.snippet_id)
            .expect("remove over limit");
        assert_eq!(manager.list_snippets(None).len(), MAX_ACTIVE_SNIPPETS);
    }

    #[test]
    fn manager_failed_persist_rolls_back_memory() {
        let mut manager = manager("rollback");
        let original_path = manager.path.clone();
        let blocker = original_path.with_extension("blocker");
        std::fs::write(&blocker, b"not a directory").expect("blocker");
        manager.path = blocker.join("snippets.json");
        assert!(matches!(
            manager.add_snippet("should fail", "value"),
            Err(SnippetError::Persistence(_))
        ));
        assert!(manager.list_snippets(None).is_empty());
    }

    #[test]
    fn manager_rejects_malformed_remote_rows_and_handles_clock_rollback() {
        let mut manager = manager("remote-validation");
        let timestamp = 1_700_000_000_000;
        let mut bad_identity = sync_record("valid", "value", timestamp);
        bad_identity.normalized_trigger = "wrong".to_string();
        assert!(matches!(
            manager.reconcile_records(vec![bad_identity]),
            Err(SnippetError::InvalidSyncIdentity)
        ));
        let mut bad_tombstone = sync_record("valid", "secret", timestamp);
        bad_tombstone.deleted_at_utc_ms = Some(timestamp);
        assert!(matches!(
            manager.reconcile_records(vec![bad_tombstone]),
            Err(SnippetError::TombstoneContainsExpansion)
        ));
        let too_old = sync_record("old", "value", MIN_REASONABLE_TIMESTAMP_UTC_MS - 1);
        assert!(matches!(
            manager.reconcile_records(vec![too_old]),
            Err(SnippetError::InvalidSyncTimestamps)
        ));
        let too_future = sync_record(
            "future",
            "value",
            now_utc_ms() + MAX_FUTURE_CLOCK_SKEW_MS + 1,
        );
        assert!(matches!(
            manager.reconcile_records(vec![too_future]),
            Err(SnippetError::InvalidSyncTimestamps)
        ));

        let added = manager.add_snippet("clock", "one").expect("add");
        manager
            .store
            .snippets
            .iter_mut()
            .find(|record| record.snippet_id == added.snippet_id)
            .expect("record")
            .updated_at_utc_ms = now_utc_ms() + 100_000;
        let before = manager.store.snippets[0].updated_at_utc_ms;
        manager
            .update_snippet(&added.snippet_id, "clock", "two")
            .expect("monotonic edit");
        assert!(manager.store.snippets[0].updated_at_utc_ms > before);
    }

    #[test]
    fn manager_reopens_trusted_local_records_after_large_clock_rollback() {
        let mut manager = manager("local-clock-rollback");
        manager.add_snippet("clock safe", "value").expect("add");
        let future = now_utc_ms() + 24 * 60 * 60 * 1_000;
        manager.store.snippets[0].created_at_utc_ms = future;
        manager.store.snippets[0].updated_at_utc_ms = future;
        manager.persist().expect("persist future local timestamp");

        let reopened = SnippetManager::from_paths(&manager.path, &manager._key_path)
            .expect("trusted local store must reopen");
        assert_eq!(reopened.list_snippets(None).len(), 1);
    }

    #[test]
    fn manager_load_sanitizes_invalid_and_duplicate_records_instead_of_failing() {
        let mut manager = manager("load-sanitize");
        let kept = manager.add_snippet("valid trigger", "kept").expect("add");
        // Simulate records written by a different app version: a trigger that
        // is reserved under the current vocabulary, and a duplicate identity
        // where the newer record must win.
        manager.store.snippets.push(StoredSnippetRecord {
            snippet_id: "vs-reserved".to_string(),
            trigger: "new line".to_string(),
            normalized_trigger: "new line".to_string(),
            expansion: "reserved now".to_string(),
            created_at_utc_ms: 1_700_000_000_000,
            updated_at_utc_ms: 1_700_000_000_000,
            deleted_at_utc_ms: None,
        });
        manager.store.snippets.push(StoredSnippetRecord {
            snippet_id: "vs-dupe".to_string(),
            trigger: "Valid Trigger".to_string(),
            normalized_trigger: "valid trigger".to_string(),
            expansion: "duplicate winner".to_string(),
            created_at_utc_ms: 1_700_000_000_000,
            updated_at_utc_ms: kept.updated_at_utc_ms + 10,
            deleted_at_utc_ms: None,
        });
        manager.persist().expect("persist mixed store");

        let reopened = SnippetManager::from_paths(&manager.path, &manager._key_path)
            .expect("sanitized load must not brick startup");
        let snippets = reopened.list_snippets(None);
        assert_eq!(snippets.len(), 1);
        assert_eq!(snippets[0].expansion, "duplicate winner");
    }

    #[test]
    fn manager_recovers_valid_backup_when_primary_is_corrupt() {
        let mut manager = manager("backup-recovery");
        manager.add_snippet("safe reply", "value").expect("add");
        let backup = atomic_file::sibling_with_suffix(&manager.path, ".bak");
        std::fs::copy(&manager.path, &backup).expect("backup");
        std::fs::write(&manager.path, b"not an encrypted envelope").expect("corrupt primary");

        let reopened =
            SnippetManager::from_paths(&manager.path, &manager._key_path).expect("recover backup");
        assert_eq!(reopened.list_snippets(None)[0].trigger, "safe reply");
        assert!(!backup.exists());
    }
}

#[cfg(test)]
mod expansion_tests {
    use super::*;

    fn snippet(trigger: &str, expansion: &str) -> VoiceSnippet {
        VoiceSnippet {
            snippet_id: format!("vs-{trigger}"),
            trigger: trigger.to_string(),
            normalized_trigger: normalize_trigger_identity(trigger).expect("trigger"),
            expansion: expansion.to_string(),
            created_at_utc_ms: 1,
            updated_at_utc_ms: 1,
        }
    }

    fn inline(source: &str, snippets: &[VoiceSnippet]) -> SnippetExpansionPlan {
        match SnippetExpansionPlan::protect(source, snippets).expect("protect") {
            ProtectionOutcome::Inline(plan) => plan,
            _ => panic!("expected inline protection"),
        }
    }

    #[test]
    fn expansion_matches_unicode_case_nfc_whitespace_and_preserves_source_spans() {
        let snippets = vec![
            snippet("Caf\u{e9} Tool", "CAFE"),
            snippet("ПРИВЕТ", "HELLO"),
        ];
        let source = "Use CAFE\u{301}\u{2003}tool, then привет!";
        let plan = inline(source, &snippets);
        let restored = plan.restore(plan.protected_text()).expect("restore");
        assert_eq!(restored, "Use CAFE, then HELLO!");
        assert_eq!(plan.source_text(), source);
        assert_eq!(plan.match_count(), 2);
    }

    #[test]
    fn expansion_obeys_boundaries_longest_overlap_and_repetition() {
        let snippets = vec![
            snippet("work", "SHORT"),
            snippet("work email", "LONG"),
            snippet("tag", "X"),
        ];
        let plan = inline("work email; tag tag; teamwork", &snippets);
        assert_eq!(
            plan.restore(plan.protected_text()).expect("restore"),
            "LONG; X X; teamwork"
        );
        assert_eq!(plan.match_count(), 3);
    }

    #[test]
    fn expansion_exact_only_allows_one_terminal_mark() {
        let snippets = vec![snippet("my reply", "Exact\n\tText")];
        match SnippetExpansionPlan::protect("  MY REPLY.  ", &snippets).expect("protect") {
            ProtectionOutcome::ExactOnly { expansion } => assert_eq!(expansion, "Exact\n\tText"),
            _ => panic!("expected exact-only"),
        }
        assert!(matches!(
            SnippetExpansionPlan::protect("my reply?!", &snippets).expect("protect"),
            ProtectionOutcome::Inline(_)
        ));
    }

    #[test]
    fn expansion_exact_only_prefers_a_trigger_that_owns_terminal_punctuation() {
        let snippets = vec![snippet("reply", "PLAIN"), snippet("reply.", "PUNCTUATED")];
        match SnippetExpansionPlan::protect("reply.", &snippets).expect("protect") {
            ProtectionOutcome::ExactOnly { expansion } => assert_eq!(expansion, "PUNCTUATED"),
            _ => panic!("expected exact-only"),
        }
    }

    #[test]
    fn expansion_restores_multiline_without_recursive_scanning() {
        let snippets = vec![
            snippet("first", "second\n\tLiteral"),
            snippet("second", "RECURSIVE"),
        ];
        let plan = inline("use first here", &snippets);
        assert_eq!(
            plan.restore(plan.protected_text()).expect("restore"),
            "use second\n\tLiteral here"
        );
    }

    #[test]
    fn expansion_rejects_deleted_duplicated_mutated_and_invented_slots() {
        let snippets = vec![snippet("trigger", "value")];
        let plan = inline("use trigger here", &snippets);
        let slot = &plan.slots[0].0;
        assert_eq!(
            plan.validate_candidate(&plan.protected_text.replace(slot, "")),
            Err(ProtectionError::InvalidSlots)
        );
        assert_eq!(
            plan.validate_candidate(&format!("{} {slot}", plan.protected_text)),
            Err(ProtectionError::InvalidSlots)
        );
        assert_eq!(
            plan.validate_candidate(&plan.protected_text.replace("VW_SNIP", "VW-CHANGED")),
            Err(ProtectionError::InvalidSlots)
        );
        assert_eq!(
            plan.validate_candidate(&format!(
                "{} {SLOT_PREFIX}invented{SLOT_SUFFIX}",
                plan.protected_text
            )),
            Err(ProtectionError::InvalidSlots)
        );
    }

    #[test]
    fn expansion_refuses_seventeenth_occurrence_without_partial_output() {
        let snippets = vec![snippet("tag", "X")];
        let source = std::iter::repeat_n("tag", MAX_EXPANSIONS_PER_DICTATION + 1)
            .collect::<Vec<_>>()
            .join(" ");
        assert!(matches!(
            SnippetExpansionPlan::protect(&source, &snippets),
            Err(ProtectionError::TooManyMatches)
        ));
    }

    #[test]
    fn expansion_does_not_match_inside_combining_or_larger_words() {
        let snippets = vec![snippet("e", "X"), snippet("cat", "Y")];
        assert!(matches!(
            SnippetExpansionPlan::protect("e\u{301} concatenate", &snippets).expect("protect"),
            ProtectionOutcome::NoMatch
        ));
    }
}
