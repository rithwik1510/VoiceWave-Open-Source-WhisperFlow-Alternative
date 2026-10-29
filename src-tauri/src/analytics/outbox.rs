//! The persisted queue of events waiting to be sent (plan 3.7):
//! `<data_dir>/analytics-outbox.jsonl`, one entry per line, so events survive
//! quitting, a crash, or days offline in the tray, and still land on the day
//! they happened. Queueing appends one line; the whole file is rewritten
//! (atomically) only when entries leave it: after a send, a prune, or a load
//! that found a torn line.

use crate::atomic_file;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{io::Write, path::PathBuf};

/// Past this, the oldest entries are dropped.
const MAX_ENTRIES: usize = 500;
/// Entries queued longer ago than this are dropped at load and before each
/// send. By queue time, not event time, so a day's summary queued days late
/// still goes out.
const MAX_AGE_MS: u64 = 14 * 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutboxEntry {
    /// UUID v4, PostHog's event uuid, which dedupes a resend of the entry.
    pub uuid: String,
    pub event: String,
    /// RFC 3339, when it happened.
    pub timestamp: String,
    pub queued_ms: u64,
    /// Base and event properties, resolved when queued.
    pub properties: Map<String, Value>,
}

pub struct Outbox {
    path: PathBuf,
    /// Oldest first.
    entries: Vec<OutboxEntry>,
}

impl Outbox {
    /// A line that doesn't parse (the last one, torn by a crash mid-append)
    /// is dropped, and the file rewritten without it.
    pub fn load(path: PathBuf, now_ms: u64) -> Self {
        let raw = atomic_file::read_to_string_recovering(&path)
            .unwrap_or_else(|err| {
                tracing::warn!("analytics: couldn't read the outbox: {err}");
                None
            })
            .unwrap_or_default();
        let lines: Vec<&str> = raw.lines().filter(|line| !line.trim().is_empty()).collect();
        let entries: Vec<OutboxEntry> = lines
            .iter()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();
        let torn = entries.len() != lines.len();
        let mut outbox = Self { path, entries };
        if outbox.prune(now_ms) || torn {
            outbox.save();
        }
        outbox
    }

    /// Drops entries past the age limit and the oldest past the cap. Returns
    /// whether anything was dropped.
    fn prune(&mut self, now_ms: u64) -> bool {
        let before = self.entries.len();
        self.entries
            .retain(|entry| now_ms.saturating_sub(entry.queued_ms) <= MAX_AGE_MS);
        let overflow = self.entries.len().saturating_sub(MAX_ENTRIES);
        self.entries.drain(..overflow);
        self.entries.len() != before
    }

    /// Appends one line. Only an outbox over its cap (weeks offline) is
    /// rewritten here.
    pub fn push(&mut self, entry: OutboxEntry) {
        let queued_ms = entry.queued_ms;
        let line = serde_json::to_string(&entry);
        self.entries.push(entry);
        if self.entries.len() > MAX_ENTRIES {
            self.prune(queued_ms);
            self.save();
            return;
        }
        let appended = line.map_err(std::io::Error::from).and_then(|line| {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)?
                .write_all(format!("{line}\n").as_bytes())
        });
        if let Err(err) = appended {
            tracing::warn!("analytics: couldn't append to the outbox: {err}");
            // A partial line would swallow the next one: rewrite instead.
            self.save();
        }
    }

    /// Up to `max` entries, oldest first. Prunes first.
    pub fn oldest(&mut self, now_ms: u64, max: usize) -> Vec<OutboxEntry> {
        if self.prune(now_ms) {
            self.save();
        }
        self.entries.iter().take(max).cloned().collect()
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
        let mut raw = String::new();
        for entry in &self.entries {
            match serde_json::to_string(entry) {
                Ok(line) => {
                    raw.push_str(&line);
                    raw.push('\n');
                }
                Err(err) => tracing::warn!("analytics: couldn't encode an outbox entry: {err}"),
            }
        }
        if let Err(err) = atomic_file::atomic_write(&self.path, raw.as_bytes()) {
            tracing::warn!("analytics: couldn't save the outbox: {err}");
        }
    }
}
