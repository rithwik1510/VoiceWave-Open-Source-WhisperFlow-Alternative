//! Sends a batch of outbox entries to PostHog's capture API (`/batch/`) and
//! sorts the response into what the outbox should do next (plan 3.7).

use super::outbox::OutboxEntry;
use serde_json::{json, Value};

/// EU Cloud (chosen 2026-10-28); the project discards client IPs.
const BATCH_URL: &str = "https://eu.i.posthog.com/batch/";

pub enum SendOutcome {
    /// 2xx: remove the entries.
    Delivered(u16),
    /// 400, 401, 403, 413: they can never succeed, so remove them too.
    Rejected(u16),
    /// Network error, 408, 429, 5xx (or anything else unexpected): keep them
    /// for the next pass. The reason is for the log.
    Retry(String),
}

/// POSTs `entries` as one batch.
pub fn send(
    agent: &ureq::Agent,
    api_key: &str,
    distinct_id: &str,
    entries: &[OutboxEntry],
) -> SendOutcome {
    let batch: Vec<Value> = entries
        .iter()
        .map(|entry| {
            json!({
                "event": entry.event,
                "distinct_id": distinct_id,
                "properties": entry.properties,
                "timestamp": entry.timestamp,
                "uuid": entry.uuid,
            })
        })
        .collect();
    let body = json!({ "api_key": api_key, "batch": batch }).to_string();

    match agent
        .post(BATCH_URL)
        .set("Content-Type", "application/json")
        .send_string(&body)
    {
        Ok(response) if (200..300).contains(&response.status()) => {
            SendOutcome::Delivered(response.status())
        }
        Ok(response) => SendOutcome::Retry(format!("HTTP {}", response.status())),
        Err(ureq::Error::Status(code @ (400 | 401 | 403 | 413), _)) => SendOutcome::Rejected(code),
        Err(ureq::Error::Status(code, _)) => SendOutcome::Retry(format!("HTTP {code}")),
        // The kind only: the transport's Display embeds the URL and detail
        // that the log doesn't need.
        Err(ureq::Error::Transport(transport)) => SendOutcome::Retry(transport.kind().to_string()),
    }
}
