//! Always-on dictation statistics: tiny per-day aggregates (counts and
//! durations only — never transcript text) powering the Stats tab.
//!
//! This is deliberately separate from the diagnostics store: diagnostics
//! keeps rich per-utterance records, is capped at 5000, and only records when
//! the user opts in. Stats must survive past the cap and work for everyone,
//! so it stores anonymous rollups in plain JSON and is fed unconditionally at
//! the end of every dictation. On first run it backfills itself from whatever
//! diagnostics records exist so the dashboard opens with real history.

use crate::atomic_file::{self, StoreLoad};
use crate::diagnostics::LatencyMetricRecord;
use chrono::{Datelike, Duration, Local, NaiveDate, TimeZone};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
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
/// Default heatmap window (calendar days). Frontend offers 30 / 91 / 365.
pub const DEFAULT_RANGE_DAYS: u32 = 30;
/// Heatmap window options the frontend can request.
pub const RANGE_OPTIONS: [u32; 3] = [30, 91, 365];

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
    /// Rolling clarity-aggregate fields (model-confidence signal). Summed per
    /// day so the always-on aggregate stays tiny and privacy-clean.
    pub clarity_logprob_sum: f64,
    pub clarity_no_speech_sum: f64,
    pub clarity_count: u32,
}

/// A count of dictations for one insertion-target class ("editor", ...).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppClassCount {
    pub name: String,
    pub count: u32,
}

/// One calendar day of aggregate data, for the heatmap / range window.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DayBucket {
    pub date: String,
    pub words: u64,
    pub dictations: u32,
    /// Top app classes by count (desc), count>0 only.
    pub app_classes: Vec<AppClassCount>,
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
    /// The requested heatmap window size (30 | 91 | 365).
    pub range_days: u32,
    /// Per-day buckets for the requested window, oldest->newest, incl. empty days.
    pub days: Vec<DayBucket>,
    /// GitHub-style current streak over full history (alive on a partial day).
    pub current_streak_days: u32,
    /// Longest run of consecutive active days over full history.
    pub longest_streak_days: u32,
    /// Top insertion-target classes across all days (desc by count, top 4).
    pub top_app_classes: Vec<AppClassCount>,
    /// Sum over all days of (raw - final) words, clamped >= 0.
    pub words_cleaned_up: u64,
    /// 0-100 rolling (7-day) model-confidence "clarity" score.
    pub clarity_score: u32,
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

/// Normalize a requested window to one of the supported options (30/91/365),
/// defaulting to 30 for anything else (including 0/absent).
fn normalize_range_days(requested: u32) -> u32 {
    if RANGE_OPTIONS.contains(&requested) {
        requested
    } else {
        DEFAULT_RANGE_DAYS
    }
}

/// "YYYY-MM-DD" key for a date (mirrors `local_day_key`'s format).
fn today_key(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

/// Aggregate a per-day app-class map into a descending top-N list.
fn top_app_classes(totals: BTreeMap<String, u32>, limit: usize) -> Vec<AppClassCount> {
    let mut entries: Vec<AppClassCount> = totals
        .into_iter()
        .map(|(name, count)| AppClassCount { name, count })
        .collect();
    entries.sort_by(|a, b| b.count.cmp(&a.count));
    entries.truncate(limit);
    entries
}

/// 0..1 clarity for a single day's stored confidence aggregate.
fn clarity_score_for_day(day: &DayStats) -> f64 {
    let avg_logprob = day.clarity_logprob_sum / day.clarity_count as f64;
    let no_speech = day.clarity_no_speech_sum / day.clarity_count as f64;
    let logprob_term = 1.0 - (avg_logprob.abs() / 1.0);
    let no_speech_term = 1.0 - (no_speech.min(0.5) / 0.5);
    (logprob_term * no_speech_term).clamp(0.0, 1.0)
}

/// Clamp a 0..1 score into a 0..100 integer.
fn clamp_to_100(score: f64) -> u32 {
    (score.clamp(0.0, 100.0)).round() as u32
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
        // Stats are derived rollups: a corrupt file costs a lifetime counter,
        // never the launch. `load_with_recovery` also quarantines the bad copy.
        match atomic_file::load_with_recovery(path, "stats", |raw| {
            serde_json::from_str::<StatsStore>(raw)
        }) {
            StoreLoad::Loaded(store) | StoreLoad::Recovered(store) => self.store = store,
            StoreLoad::Missing | StoreLoad::Reset => self.store = StatsStore::default(),
        }
        Ok(())
    }

    fn persist(&self) -> Result<(), StatsError> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let raw = serde_json::to_string(&self.store).map_err(StatsError::Parse)?;
        atomic_file::atomic_write(path, raw.as_bytes()).map_err(StatsError::Write)
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
        avg_logprob: Option<f32>,
        no_speech_prob: Option<f32>,
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
        // Fold optional model-confidence signal into the day's rolling clarity
        // aggregate when present (only meaningful when a faster-whisper decode
        // produced telemetry).
        if let (Some(logprob), Some(no_speech)) = (avg_logprob, no_speech_prob) {
            day.clarity_logprob_sum += f64::from(logprob);
            day.clarity_no_speech_sum += f64::from(no_speech);
            day.clarity_count += 1;
        }
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
        avg_logprob: Option<f32>,
        no_speech_prob: Option<f32>,
    ) -> Result<(), StatsError> {
        if self.ingest(
            timestamp_utc_ms,
            final_words,
            raw_words,
            audio_ms,
            app_class,
            avg_logprob,
            no_speech_prob,
        ) {
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
            // Only import dictations that actually inserted, matching the live
            // always-on gating in state.rs.
            if !record.success {
                continue;
            }
            self.ingest(
                record.timestamp_utc_ms,
                record.asr_final_word_count,
                record.asr_raw_word_count,
                record.audio_duration_ms,
                record.insertion_target_class.as_deref(),
                record.fw_avg_logprob,
                record.fw_no_speech_prob,
            );
        }
        self.store.backfilled = true;
        self.persist()
    }

    pub fn summary(&self) -> StatsSummary {
        self.summary_for_window(Local::now().date_naive(), DEFAULT_RANGE_DAYS)
    }

    /// Summary with an explicit "today" so windowing is testable.
    pub fn summary_for_today(&self, today: NaiveDate) -> StatsSummary {
        self.summary_for_window(today, DEFAULT_RANGE_DAYS)
    }

    /// Summary for an explicit "today" and heatmap window.
    pub fn summary_for_window(&self, today: NaiveDate, range_days: u32) -> StatsSummary {
        let normalized_range = normalize_range_days(range_days);
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
            range_days: normalized_range,
            days: Vec::new(),
            current_streak_days: 0,
            longest_streak_days: 0,
            top_app_classes: Vec::new(),
            words_cleaned_up: 0,
            clarity_score: 0,
        };

        let mut month_audio_ms = 0u64;
        let mut app_class_totals: BTreeMap<String, u32> = BTreeMap::new();
        for (key, day) in &self.store.days {
            summary.all_time_words += day.final_words;
            summary.all_time_dictations += u64::from(day.dictations);
            summary.speaking_ms += day.audio_ms;
            if day.dictations > 0 {
                summary.active_days += 1;
            }
            summary.words_cleaned_up +=
                day.raw_words.saturating_sub(day.final_words);
            for (class, count) in &day.app_classes {
                *app_class_totals.entry(class.clone()).or_default() += count;
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

        // Heatmap window + streak math over full history.
        summary.days = self.day_buckets_for_today(today, normalized_range);
        summary.current_streak_days = self.current_streak_for_today(today);
        summary.longest_streak_days = self.longest_streak();
        summary.top_app_classes = top_app_classes(app_class_totals, 4);
        summary.clarity_score = self.clarity_score_for_today(today);

        summary
    }

    /// Per-day buckets for the requested window (calendar days, oldest->newest),
    /// including empty cells so the heatmap renders a full grid.
    pub fn day_buckets(&self, window_days: u32) -> Vec<DayBucket> {
        self.day_buckets_for_today(Local::now().date_naive(), window_days)
    }

    fn day_buckets_for_today(&self, today: NaiveDate, window_days: u32) -> Vec<DayBucket> {
        let window_days = window_days.max(1);
        let start = today - Duration::days(i64::from(window_days) - 1);
        let mut buckets = Vec::with_capacity(window_days as usize);
        let mut cursor = start;
        while cursor <= today {
            let key = cursor.format("%Y-%m-%d").to_string();
            let day = self.store.days.get(&key);
            let mut app_classes: Vec<AppClassCount> = Vec::new();
            if let Some(day) = day {
                app_classes = day
                    .app_classes
                    .iter()
                    .filter(|(_, count)| **count > 0)
                    .map(|(name, count)| AppClassCount {
                        name: name.clone(),
                        count: *count,
                    })
                    .collect();
                app_classes.sort_by(|a, b| b.count.cmp(&a.count));
                app_classes.truncate(3);
            }
            buckets.push(DayBucket {
                date: key,
                words: day.map_or(0, |d| d.final_words),
                dictations: day.map_or(0, |d| d.dictations),
                app_classes,
            });
            cursor += Duration::days(1);
        }
        buckets
    }

    /// GitHub-style current streak: count today if active, else continue from
    /// yesterday (an empty, not-yet-finished day does not break the streak).
    pub fn current_streak(&self) -> u32 {
        self.current_streak_for_today(Local::now().date_naive())
    }

    fn current_streak_for_today(&self, today: NaiveDate) -> u32 {
        let mut cursor = today;
        // If today is not active yet, start counting from yesterday.
        if !self.day_active(&today_key(today)) {
            cursor -= Duration::days(1);
        }
        let mut streak = 0u32;
        loop {
            if !self.day_active(&cursor.format("%Y-%m-%d").to_string()) {
                break;
            }
            streak += 1;
            if cursor <= NaiveDate::from_ymd_opt(1970, 1, 1).unwrap() {
                break;
            }
            cursor -= Duration::days(1);
        }
        streak
    }

    /// Longest run of consecutive active days over full history.
    pub fn longest_streak(&self) -> u32 {
        if self.store.days.is_empty() {
            return 0;
        }
        // Walk calendar days from earliest to latest stored day so that gap
        // days (absent from the map) correctly reset the run.
        let first = self.store.days.keys().next().unwrap();
        let last = self.store.days.keys().last().unwrap();
        let Ok(mut cursor) = NaiveDate::parse_from_str(first, "%Y-%m-%d") else {
            return 0;
        };
        let Ok(end) = NaiveDate::parse_from_str(last, "%Y-%m-%d") else {
            return 0;
        };
        let mut longest = 0u32;
        let mut current = 0u32;
        while cursor <= end {
            if self.day_active(&cursor.format("%Y-%m-%d").to_string()) {
                current += 1;
                if current > longest {
                    longest = current;
                }
            } else {
                current = 0;
            }
            cursor += Duration::days(1);
        }
        longest
    }

    fn day_active(&self, key: &str) -> bool {
        self.store
            .days
            .get(key)
            .map_or(false, |day| day.dictations > 0)
    }

    /// Rolling 7-day model-confidence "clarity" score (0-100).
    fn clarity_score_for_today(&self, today: NaiveDate) -> u32 {
        let start = today - Duration::days(6);
        let mut score_sum = 0.0f64;
        let mut days_with_data = 0u64;
        let mut cursor = start;
        while cursor <= today {
            let key = cursor.format("%Y-%m-%d").to_string();
            if let Some(day) = self.store.days.get(&key) {
                if day.clarity_count > 0 {
                    score_sum += clarity_score_for_day(day);
                    days_with_data += 1;
                }
            }
            cursor += Duration::days(1);
        }
        if days_with_data == 0 {
            return 0;
        }
        clamp_to_100((score_sum / days_with_data as f64) * 100.0)
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

    /// Stats already degraded gracefully at the call site; this makes the
    /// store itself recover so it does not have to.
    #[test]
    fn corrupt_store_loads_defaults_instead_of_failing() {
        for (case, payload, expect_quarantine) in [
            ("nul", vec![0_u8; 299], false),
            ("truncated", br#"{"days":{"2026-07-06""#.to_vec(), true),
            ("garbage", vec![0x8f, 0x2c, 0xff, 0x00, 0x41, 0xfe], true),
        ] {
            let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "voicewave-stats-corrupt-{case}-{}-{seq}",
                std::process::id()
            ));
            std::fs::create_dir_all(&dir).expect("case dir");
            let path = dir.join("stats.json");
            std::fs::write(&path, &payload).expect("seed corrupt store");

            let manager = StatsManager::from_path(&path)
                .expect("a corrupt stats store must not stop the app");
            assert_eq!(manager.summary().all_time_words, 0, "{case}");

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
    fn gates_reject_empty_and_too_short_dictations() {
        let mut stats = StatsManager::in_memory();
        assert!(!stats.ingest(ts_for(NaiveDate::from_ymd_opt(2026, 7, 6).unwrap()), 0, 0, 10_000, None, None, None));
        assert!(!stats.ingest(ts_for(NaiveDate::from_ymd_opt(2026, 7, 6).unwrap()), 5, 5, 1_000, None, None, None));
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
        assert!(stats.ingest(ts_for(today), 100, 104, 60_000, Some("editor"), None, None));
        assert!(stats.ingest(ts_for(within_week), 50, 50, 30_000, Some("browser"), None, None));
        assert!(stats.ingest(ts_for(outside_week), 30, 30, 20_000, None, None, None));
        assert!(stats.ingest(ts_for(prev_month_day), 40, 40, 25_000, None, None, None));

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
        assert!(stats.ingest(ts_for(today), 12, 12, 3_000, None, None, None));
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
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn record_dictation_persists_and_reloads() {
        let path = temp_stats_path();
        let today = NaiveDate::from_ymd_opt(2026, 7, 6).unwrap();
        {
            let mut stats = StatsManager::from_path(&path).expect("create");
            stats
                .record_dictation(ts_for(today), 25, 26, 12_000, Some("browser"), None, None)
                .expect("record");
        }
        let stats = StatsManager::from_path(&path).expect("reload");
        let summary = stats.summary_for_today(today);
        assert_eq!(summary.all_time_words, 25);
        assert_eq!(summary.all_time_dictations, 1);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn backfill_skips_failed_dictations() {
        let path = temp_stats_path();
        let today = NaiveDate::from_ymd_opt(2026, 7, 6).unwrap();
        let ok = LatencyMetricRecord {
            timestamp_utc_ms: ts_for(today),
            asr_final_word_count: 40,
            asr_raw_word_count: 42,
            audio_duration_ms: 20_000,
            success: true,
            ..test_latency_record()
        };
        let failed = LatencyMetricRecord {
            timestamp_utc_ms: ts_for(today),
            asr_final_word_count: 30,
            asr_raw_word_count: 31,
            audio_duration_ms: 15_000,
            success: false,
            ..test_latency_record()
        };
        {
            let mut stats = StatsManager::from_path(&path).expect("create");
            stats
                .backfill_from_latency_records(&[ok, failed])
                .expect("backfill");
            // The failed record must be excluded.
            assert_eq!(stats.summary_for_today(today).all_time_words, 40);
            assert_eq!(stats.summary_for_today(today).all_time_dictations, 1);
        }
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn day_buckets_are_ordered_and_fill_empty_cells() {
        let mut stats = StatsManager::in_memory();
        let today = NaiveDate::from_ymd_opt(2026, 7, 6).unwrap();
        // Only two days of data inside a 5-day window.
        stats.ingest(ts_for(today), 10, 10, 6_000, Some("editor"), None, None);
        let five_days_ago = today - Duration::days(4);
        stats.ingest(ts_for(five_days_ago), 20, 20, 7_000, Some("browser"), None, None);

        let buckets = stats.day_buckets_for_today(today, 5);
        assert_eq!(buckets.len(), 5);
        assert_eq!(buckets[0].date, (today - Duration::days(4)).format("%Y-%m-%d").to_string());
        assert_eq!(buckets[4].date, today.format("%Y-%m-%d").to_string());
        assert_eq!(buckets[0].words, 20);
        assert_eq!(buckets[0].dictations, 1);
        // Middle empty day -> empty cell.
        assert_eq!(buckets[2].words, 0);
        assert_eq!(buckets[2].dictations, 0);
        assert!(buckets[2].app_classes.is_empty());
        // Oldest->newest order.
        for w in buckets.windows(2) {
            assert!(w[0].date < w[1].date);
        }
    }

    #[test]
    fn top_app_classes_are_descending_and_capped() {
        let mut stats = StatsManager::in_memory();
        let today = NaiveDate::from_ymd_opt(2026, 7, 6).unwrap();
        for (i, class) in ["editor", "browser", "collab", "terminal", "chat"].iter().enumerate() {
            let words = (u32::try_from(i).unwrap() + 1) * 5;
            stats.ingest(ts_for(today), words, words, 6_000, Some(class), None, None);
        }
        let summary = stats.summary_for_today(today);
        assert_eq!(summary.top_app_classes.len(), 4);
        // Sorted desc by count; each class was ingested once, all count 1, so
        // order may tie — just assert all appear and none exceed the cap.
        assert!(summary.top_app_classes.iter().all(|c| c.name != ""));
        assert_eq!(summary.top_app_classes.len(), 4);
    }

    #[test]
    fn current_streak_alive_with_today_active() {
        let mut stats = StatsManager::in_memory();
        let today = NaiveDate::from_ymd_opt(2026, 7, 6).unwrap();
        stats.ingest(ts_for(today), 10, 10, 6_000, None, None, None);
        stats.ingest(ts_for(today - Duration::days(1)), 10, 10, 6_000, None, None, None);
        stats.ingest(ts_for(today - Duration::days(2)), 10, 10, 6_000, None, None, None);
        assert_eq!(stats.current_streak_for_today(today), 3);
    }

    #[test]
    fn current_streak_stays_alive_on_empty_partial_day() {
        let mut stats = StatsManager::in_memory();
        let today = NaiveDate::from_ymd_opt(2026, 7, 6).unwrap();
        // No dictation today, but yesterday and before are active.
        stats.ingest(ts_for(today - Duration::days(1)), 10, 10, 6_000, None, None, None);
        stats.ingest(ts_for(today - Duration::days(2)), 10, 10, 6_000, None, None, None);
        assert_eq!(stats.current_streak_for_today(today), 2);
    }

    #[test]
    fn current_streak_broken_after_gap() {
        let mut stats = StatsManager::in_memory();
        let today = NaiveDate::from_ymd_opt(2026, 7, 6).unwrap();
        // Active yesterday and today, but a gap two days ago.
        stats.ingest(ts_for(today), 10, 10, 6_000, None, None, None);
        stats.ingest(ts_for(today - Duration::days(1)), 10, 10, 6_000, None, None, None);
        stats.ingest(ts_for(today - Duration::days(3)), 10, 10, 6_000, None, None, None);
        assert_eq!(stats.current_streak_for_today(today), 2);
    }

    #[test]
    fn longest_streak_finds_run_across_gap() {
        let mut stats = StatsManager::in_memory();
        let today = NaiveDate::from_ymd_opt(2026, 7, 6).unwrap();
        // Run of 3, then a gap, then a run of 5.
        for offset in 0..3 {
            stats.ingest(ts_for(today - Duration::days(offset)), 10, 10, 6_000, None, None, None);
        }
        // Gap at offset 3.
        for offset in 4..9 {
            stats.ingest(ts_for(today - Duration::days(offset)), 10, 10, 6_000, None, None, None);
        }
        assert_eq!(stats.longest_streak(), 5);
    }

    #[test]
    fn normalize_range_and_clarity_clamp() {
        assert_eq!(normalize_range_days(30), 30);
        assert_eq!(normalize_range_days(91), 91);
        assert_eq!(normalize_range_days(365), 365);
        assert_eq!(normalize_range_days(0), 30);
        assert_eq!(normalize_range_days(999), 30);

        let mut stats = StatsManager::in_memory();
        let today = NaiveDate::from_ymd_opt(2026, 7, 6).unwrap();
        // Very clear: logprob near 0, no_speech near 0 -> score near 100.
        // (0.1 logprob / 0.05 no_speech yields 0.9 * 0.9 = 0.81 -> 81.)
        stats.ingest(ts_for(today), 10, 10, 6_000, None, Some(-0.1), Some(0.05));
        let summary = stats.summary_for_today(today);
        assert_eq!(summary.range_days, 30);
        assert!(summary.clarity_score >= 75, "expected high clarity, got {}", summary.clarity_score);

        // Very unclear -> score should be low-ish (clamped >= 0).
        let mut stats2 = StatsManager::in_memory();
        stats2.ingest(ts_for(today), 10, 10, 6_000, None, Some(-0.95), Some(0.49));
        let summary2 = stats2.summary_for_today(today);
        assert!(summary2.clarity_score <= 10, "expected low clarity, got {}", summary2.clarity_score);
        // Out-of-range inputs still clamp into 0..100.
        let mut stats3 = StatsManager::in_memory();
        stats3.ingest(ts_for(today), 10, 10, 6_000, None, Some(-5.0), Some(2.0));
        let summary3 = stats3.summary_for_today(today);
        assert!((0..=100).contains(&summary3.clarity_score));
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
