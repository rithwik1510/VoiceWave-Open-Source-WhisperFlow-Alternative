//! Always-on dictation statistics: tiny per-day aggregates (counts and
//! durations only — never transcript text) powering the Stats tab.
//!
//! This is deliberately separate from the diagnostics store: diagnostics
//! keeps rich per-utterance records, is capped at 5000, and only records when
//! the user opts in. Stats must survive past the cap and work for everyone,
//! so it stores anonymous rollups in plain JSON and is fed unconditionally at
//! the end of every dictation. On first run it backfills itself from whatever
//! diagnostics records exist so the dashboard opens with real history.

use crate::diagnostics::LatencyMetricRecord;
use chrono::{Datelike, Duration, Local, NaiveDate, TimeZone};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

/// Average keyboard typing speed used for the "time saved" estimate. Shown in
/// the UI footnote so the math stays honest.
pub const TYPING_BASELINE_WPM: u64 = 40;
/// Dictations shorter than this are ignored: WPM math on a one-word burst is
/// noise, and they barely move totals.
const MIN_COUNTED_AUDIO_MS: u64 = 2_000;
/// Personal-best WPM only counts sustained dictations.
const BEST_WPM_MIN_AUDIO_MS: u64 = 5_000;
const BEST_WPM_MIN_WORDS: u32 = 8;

#[derive(Debug, thiserror::Error)]
pub enum StatsError {
    #[error("failed to read stats: {0}")]
    Read(std::io::Error),
    #[error("failed to write stats: {0}")]
    Write(std::io::Error),
    #[error("failed to parse stats JSON: {0}")]
    Parse(serde_json::Error),
    #[error("cannot resolve app data directory")]
    AppData,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DayStats {
    pub dictations: u32,
    pub final_words: u64,
    pub raw_words: u64,
    pub audio_ms: u64,
    /// Dictation counts per insertion target class ("editor", "browser", ...).
    pub app_classes: BTreeMap<String, u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct StatsStore {
    version: u8,
    /// One-time import from the diagnostics store already ran.
    backfilled: bool,
    best_dictation_wpm: f64,
    longest_dictation_words: u32,
    /// Keyed by local calendar day, "YYYY-MM-DD". BTreeMap keeps keys sorted
    /// so window queries are simple range scans.
    days: BTreeMap<String, DayStats>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StatsSummary {
    pub today_words: u64,
    /// Last 7 local days including today.
    pub week_words: u64,
    /// Current calendar month.
    pub month_words: u64,
    pub prev_month_words: u64,
    pub all_time_words: u64,
    pub all_time_dictations: u64,
    pub speaking_ms: u64,
    /// Duration-weighted words per minute across all counted dictations.
    pub average_wpm: f64,
    pub best_dictation_wpm: f64,
    pub time_saved_ms_all_time: u64,
    pub time_saved_ms_month: u64,
    pub typing_baseline_wpm: u64,
    pub active_days: u32,
}

pub struct StatsManager {
    /// None = in-memory fallback (stats survive the session only). Used when
    /// the store path can't be resolved; stats must never block dictation.
    path: Option<PathBuf>,
    store: StatsStore,
}

fn local_day_key(timestamp_utc_ms: u64) -> Option<String> {
    Local
        .timestamp_millis_opt(timestamp_utc_ms as i64)
        .single()
        .map(|moment| moment.format("%Y-%m-%d").to_string())
}

/// Milliseconds a 40-WPM typist would need for `words`.
fn typing_ms(words: u64) -> u64 {
    words * (60_000 / TYPING_BASELINE_WPM)
}

impl StatsManager {
    pub fn new() -> Result<Self, StatsError> {
        let proj_dirs =
            ProjectDirs::from("com", "voicewave", "localcore").ok_or(StatsError::AppData)?;
        Self::from_path(proj_dirs.config_dir().join("stats.json"))
    }

    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, StatsError> {
        let path = path.as_ref().to_path_buf();
        let mut manager = Self {
            path: Some(path),
            store: StatsStore::default(),
        };
        manager.load()?;
        Ok(manager)
    }

    pub fn in_memory() -> Self {
        Self {
            path: None,
            store: StatsStore::default(),
        }
    }

    fn load(&mut self) -> Result<(), StatsError> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if !path.exists() {
            return Ok(());
        }
        let raw = fs::read_to_string(path).map_err(StatsError::Read)?;
        self.store = serde_json::from_str(&raw).map_err(StatsError::Parse)?;
        Ok(())
    }

    fn persist(&self) -> Result<(), StatsError> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(StatsError::Write)?;
        }
        let raw = serde_json::to_string(&self.store).map_err(StatsError::Parse)?;
        fs::write(path, raw).map_err(StatsError::Write)
    }

    pub fn backfilled(&self) -> bool {
        self.store.backfilled
    }

    /// Fold one dictation into the rollups. Applies the accuracy gates; does
    /// not persist (callers decide when to write).
    fn ingest(
        &mut self,
        timestamp_utc_ms: u64,
        final_words: u32,
        raw_words: u32,
        audio_ms: u64,
        app_class: Option<&str>,
    ) -> bool {
        if final_words == 0 || audio_ms < MIN_COUNTED_AUDIO_MS {
            return false;
        }
        let Some(day_key) = local_day_key(timestamp_utc_ms) else {
            return false;
        };
        let day = self.store.days.entry(day_key).or_default();
        day.dictations += 1;
        day.final_words += u64::from(final_words);
        day.raw_words += u64::from(raw_words.max(final_words));
        day.audio_ms += audio_ms;
        if let Some(class) = app_class {
            let class = class.trim();
            if !class.is_empty() && class != "unknown" {
                *day.app_classes.entry(class.to_string()).or_default() += 1;
            }
        }
        if audio_ms >= BEST_WPM_MIN_AUDIO_MS && final_words >= BEST_WPM_MIN_WORDS {
            let wpm = f64::from(final_words) * 60_000.0 / audio_ms as f64;
            if wpm > self.store.best_dictation_wpm {
                self.store.best_dictation_wpm = wpm;
            }
        }
        if final_words > self.store.longest_dictation_words {
            self.store.longest_dictation_words = final_words;
        }
        true
    }

    pub fn record_dictation(
        &mut self,
        timestamp_utc_ms: u64,
        final_words: u32,
        raw_words: u32,
        audio_ms: u64,
        app_class: Option<&str>,
    ) -> Result<(), StatsError> {
        if self.ingest(timestamp_utc_ms, final_words, raw_words, audio_ms, app_class) {
            self.persist()?;
        }
        Ok(())
    }

    /// One-time import of existing diagnostics records (users who had opted
    /// into diagnostics get a dashboard pre-filled with their real history).
    /// Runs at most once; a fresh store with no diagnostics simply starts
    /// counting from now.
    pub fn backfill_from_latency_records(
        &mut self,
        records: &[LatencyMetricRecord],
    ) -> Result<(), StatsError> {
        if self.store.backfilled {
            return Ok(());
        }
        for record in records {
            self.ingest(
                record.timestamp_utc_ms,
                record.asr_final_word_count,
                record.asr_raw_word_count,
                record.audio_duration_ms,
                record.insertion_target_class.as_deref(),
            );
        }
        self.store.backfilled = true;
        self.persist()
    }

    pub fn summary(&self) -> StatsSummary {
        self.summary_for_today(Local::now().date_naive())
    }

    /// Summary with an explicit "today" so windowing is testable.
    pub fn summary_for_today(&self, today: NaiveDate) -> StatsSummary {
        let today_key = today.format("%Y-%m-%d").to_string();
        let week_start_key = (today - Duration::days(6)).format("%Y-%m-%d").to_string();
        let month_prefix = today.format("%Y-%m").to_string();
        let prev_month = if today.month() == 1 {
            format!("{:04}-12", today.year() - 1)
        } else {
            format!("{:04}-{:02}", today.year(), today.month() - 1)
        };

        let mut summary = StatsSummary {
            today_words: 0,
            week_words: 0,
            month_words: 0,
            prev_month_words: 0,
            all_time_words: 0,
            all_time_dictations: 0,
            speaking_ms: 0,
            average_wpm: 0.0,
            best_dictation_wpm: self.store.best_dictation_wpm,
            time_saved_ms_all_time: 0,
            time_saved_ms_month: 0,
            typing_baseline_wpm: TYPING_BASELINE_WPM,
            active_days: 0,
        };

        let mut month_audio_ms = 0u64;
        for (key, day) in &self.store.days {
            summary.all_time_words += day.final_words;
            summary.all_time_dictations += u64::from(day.dictations);
            summary.speaking_ms += day.audio_ms;
            if day.dictations > 0 {
                summary.active_days += 1;
            }
            if key == &today_key {
                summary.today_words = day.final_words;
            }
            if key.as_str() >= week_start_key.as_str() && key.as_str() <= today_key.as_str() {
                summary.week_words += day.final_words;
            }
            if key.starts_with(&month_prefix) {
                summary.month_words += day.final_words;
                month_audio_ms += day.audio_ms;
            }
            if key.starts_with(&prev_month) {
                summary.prev_month_words += day.final_words;
            }
        }

        if summary.speaking_ms > 0 {
            summary.average_wpm =
                summary.all_time_words as f64 * 60_000.0 / summary.speaking_ms as f64;
        }
        summary.time_saved_ms_all_time =
            typing_ms(summary.all_time_words).saturating_sub(summary.speaking_ms);
        summary.time_saved_ms_month =
            typing_ms(summary.month_words).saturating_sub(month_audio_ms);
        summary
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

    fn temp_stats_path() -> PathBuf {
        let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "voicewave-stats-test-{}-{}.json",
            std::process::id(),
            seq
        ))
    }

    /// A timestamp inside the given local date, safe from midnight edges.
    fn ts_for(date: NaiveDate) -> u64 {
        Local
            .from_local_datetime(&date.and_hms_opt(12, 0, 0).expect("valid time"))
            .single()
            .expect("unambiguous local noon")
            .timestamp_millis() as u64
    }

    #[test]
    fn gates_reject_empty_and_too_short_dictations() {
        let mut stats = StatsManager::in_memory();
        assert!(!stats.ingest(ts_for(NaiveDate::from_ymd_opt(2026, 7, 6).unwrap()), 0, 0, 10_000, None));
        assert!(!stats.ingest(ts_for(NaiveDate::from_ymd_opt(2026, 7, 6).unwrap()), 5, 5, 1_000, None));
        assert_eq!(stats.summary().all_time_words, 0);
    }

    #[test]
    fn summary_windows_and_derived_metrics_are_exact() {
        let mut stats = StatsManager::in_memory();
        let today = NaiveDate::from_ymd_opt(2026, 7, 6).unwrap();
        let within_week = today - Duration::days(6);
        let outside_week = today - Duration::days(7);
        let prev_month_day = NaiveDate::from_ymd_opt(2026, 6, 15).unwrap();

        // 100 words / 60s today (July 6); 50 words / 30s at the week edge
        // (June 30); 30 words / 20s just outside the week (June 29, so it
        // lands in the previous-month bucket along with June 15).
        assert!(stats.ingest(ts_for(today), 100, 104, 60_000, Some("editor")));
        assert!(stats.ingest(ts_for(within_week), 50, 50, 30_000, Some("browser")));
        assert!(stats.ingest(ts_for(outside_week), 30, 30, 20_000, None));
        assert!(stats.ingest(ts_for(prev_month_day), 40, 40, 25_000, None));

        let summary = stats.summary_for_today(today);
        assert_eq!(summary.today_words, 100);
        assert_eq!(summary.week_words, 150);
        assert_eq!(summary.month_words, 100); // July: only today's dictation
        // June 30 + June 29 + June 15 — the rolling week and the calendar
        // month overlap by design, so June 30 counts in both.
        assert_eq!(summary.prev_month_words, 120);
        assert_eq!(summary.all_time_words, 220);
        assert_eq!(summary.all_time_dictations, 4);
        assert_eq!(summary.speaking_ms, 135_000);
        assert_eq!(summary.active_days, 4);
        // Duration-weighted: 220 words / 2.25 min.
        assert!((summary.average_wpm - 220.0 * 60_000.0 / 135_000.0).abs() < 1e-9);
        // Typing 220 words at 40wpm = 330s; speaking took 135s.
        assert_eq!(summary.time_saved_ms_all_time, 330_000 - 135_000);
        // July: 100 words -> 150s typing minus 60s speaking.
        assert_eq!(summary.time_saved_ms_month, 150_000 - 60_000);
        // Best single dictation: 100 words / 60s = 100 wpm.
        assert!((summary.best_dictation_wpm - 100.0).abs() < 1e-9);
    }

    #[test]
    fn best_wpm_requires_sustained_dictation() {
        let mut stats = StatsManager::in_memory();
        let today = NaiveDate::from_ymd_opt(2026, 7, 6).unwrap();
        // 12 words in 3s would be 240wpm but is under the 5s floor.
        assert!(stats.ingest(ts_for(today), 12, 12, 3_000, None));
        assert_eq!(stats.summary_for_today(today).best_dictation_wpm, 0.0);
    }

    #[test]
    fn backfill_runs_once_and_persists_across_reload() {
        let path = temp_stats_path();
        let today = NaiveDate::from_ymd_opt(2026, 7, 6).unwrap();
        let record = LatencyMetricRecord {
            timestamp_utc_ms: ts_for(today),
            asr_final_word_count: 60,
            asr_raw_word_count: 62,
            audio_duration_ms: 30_000,
            insertion_target_class: Some("editor".to_string()),
            ..test_latency_record()
        };

        {
            let mut stats = StatsManager::from_path(&path).expect("create");
            stats
                .backfill_from_latency_records(std::slice::from_ref(&record))
                .expect("backfill");
            // Second backfill with the same record must be a no-op.
            stats
                .backfill_from_latency_records(std::slice::from_ref(&record))
                .expect("backfill again");
            assert_eq!(stats.summary_for_today(today).all_time_words, 60);
        }

        let stats = StatsManager::from_path(&path).expect("reload");
        assert!(stats.backfilled());
        assert_eq!(stats.summary_for_today(today).all_time_words, 60);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn record_dictation_persists_and_reloads() {
        let path = temp_stats_path();
        let today = NaiveDate::from_ymd_opt(2026, 7, 6).unwrap();
        {
            let mut stats = StatsManager::from_path(&path).expect("create");
            stats
                .record_dictation(ts_for(today), 25, 26, 12_000, Some("browser"))
                .expect("record");
        }
        let stats = StatsManager::from_path(&path).expect("reload");
        let summary = stats.summary_for_today(today);
        assert_eq!(summary.all_time_words, 25);
        assert_eq!(summary.all_time_dictations, 1);
        let _ = fs::remove_file(path);
    }

    /// Minimal valid LatencyMetricRecord for struct-update syntax in tests
    /// (only serde-required fields; everything else takes serde defaults).
    fn test_latency_record() -> LatencyMetricRecord {
        serde_json::from_str(
            r#"{
                "sessionId": 0, "timestampUtcMs": 0, "captureMs": 0,
                "releaseToTranscribingMs": 0, "decodeMs": 0, "postMs": 0,
                "insertMs": 0, "totalMs": 0, "audioDurationMs": 0,
                "modelId": "test", "decodeMode": "balanced",
                "watchdogRecovered": false, "segmentsCaptured": 0,
                "releaseStopDetectedAtUtcMs": 0, "modelInitMs": 0,
                "audioConditionMs": 0, "decodeComputeMs": 0,
                "runtimeCacheHit": false, "backendRequested": "cpu",
                "backendUsed": "cpu", "backendFallback": false,
                "holdToFirstDraftMs": 0, "incrementalDecodeMs": 0,
                "releaseFinalizeMs": 0, "incrementalWindowsDecoded": 0,
                "finalizeTailAudioMs": 0, "success": true
            }"#,
        )
        .expect("valid minimal LatencyMetricRecord")
    }
}
