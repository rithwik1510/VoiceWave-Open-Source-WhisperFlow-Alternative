//! The persisted queue of events waiting to be sent (plan 3.7):
//! `<data_dir>/analytics-outbox.json`, so events survive quitting, a crash,
//! or days offline in the tray, and still land on the day they happened.

use crate::atomic_file::{self, StoreLoad};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::PathBuf;

/// Past this, the oldest entries are dropped.
const MAX_ENTRIES: usize = 500;
/// Entries older than this are dropped at load and before each send.
const MAX_AGE_MS: u64 = 14 * 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutboxEntry {
    /// UUID v4, PostHog's event uuid, which dedupes a resend of the entry.
    pub uuid: String,
    pub event: String,
    /// RFC 3339, when it happened.
    pub timestamp: String,
    /// Base and event properties, resolved when queued.
    pub properties: Map<String, Value>,
    pub attempts: u32,
    pub next_at_ms: u64,
}

impl OutboxEntry {
    fn happened_at_ms(&self) -> Option<u64> {
        chrono::DateTime::parse_from_rfc3339(&self.timestamp)
            .ok()
            .and_then(|at| u64::try_from(at.timestamp_millis()).ok())
    }
}

pub struct Outbox {
    path: PathBuf,
    /// Oldest first.
    entries: Vec<OutboxEntry>,
}

impl Outbox {
    pub fn load(path: PathBuf, now_ms: u64) -> Self {
        let entries = match atomic_file::load_with_recovery(&path, "analytics outbox", |raw| {
            serde_json::from_str::<Vec<OutboxEntry>>(raw)
        }) {
            StoreLoad::Loaded(entries) | StoreLoad::Recovered(entries) => entries,
            StoreLoad::Missing | StoreLoad::Reset => Vec::new(),
        };
        let mut outbox = Self { path, entries };
        if outbox.prune(now_ms) {
            outbox.save();
        }
        outbox
    }

    /// Drops entries past the age limit (or with an unreadable time) and the
    /// oldest past the cap. Returns whether anything was dropped.
    fn prune(&mut self, now_ms: u64) -> bool {
        let before = self.entries.len();
        self.entries.retain(|entry| {
            entry
                .happened_at_ms()
                .is_some_and(|at| now_ms.saturating_sub(at) <= MAX_AGE_MS)
        });
        let overflow = self.entries.len().saturating_sub(MAX_ENTRIES);
        self.entries.drain(..overflow);
        self.entries.len() != before
    }

    pub fn push(&mut self, entry: OutboxEntry, now_ms: u64) {
        self.entries.push(entry);
        self.prune(now_ms);
        self.save();
    }

    /// Up to `max` entries that are due, oldest first. Prunes first.
    pub fn due(&mut self, now_ms: u64, max: usize) -> Vec<OutboxEntry> {
        if self.prune(now_ms) {
            self.save();
        }
        self.entries
            .iter()
            .filter(|entry| entry.next_at_ms <= now_ms)
            .take(max)
            .cloned()
            .collect()
    }

    /// Removes the sent (or permanently rejected) entries.
    pub fn remove(&mut self, uuids: &[String]) {
        let before = self.entries.len();
        self.entries.retain(|entry| !uuids.contains(&entry.uuid));
        // Unchanged after a purge raced the send: writing now would bring
        // back the file that turning sharing off just deleted.
        if self.entries.len() != before {
            self.save();
        }
    }

    /// Schedules another try for entries whose send failed.
    pub fn defer(&mut self, uuids: &[String], next_at_ms: impl Fn(u32) -> u64) {
        let mut changed = false;
        for entry in self
            .entries
            .iter_mut()
            .filter(|entry| uuids.contains(&entry.uuid))
        {
            entry.attempts = entry.attempts.saturating_add(1);
            entry.next_at_ms = next_at_ms(entry.attempts);
            changed = true;
        }
        if changed {
            self.save();
        }
    }

    /// Empties the queue and deletes its file (sharing turned off).
    pub fn purge(&mut self) {
        self.entries.clear();
        for path in [
            self.path.clone(),
            atomic_file::sibling_with_suffix(&self.path, ".bak"),
            atomic_file::sibling_with_suffix(&self.path, ".tmp"),
        ] {
            if let Err(err) = std::fs::remove_file(&path) {
                if err.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!("analytics: couldn't delete the outbox: {err}");
                }
            }
        }
    }

    fn save(&self) {
        let raw = match serde_json::to_vec(&self.entries) {
            Ok(raw) => raw,
            Err(err) => {
                tracing::warn!("analytics: couldn't encode the outbox: {err}");
                return;
            }
        };
        if let Err(err) = atomic_file::atomic_write(&self.path, &raw) {
            tracing::warn!("analytics: couldn't save the outbox: {err}");
        }
    }
}
