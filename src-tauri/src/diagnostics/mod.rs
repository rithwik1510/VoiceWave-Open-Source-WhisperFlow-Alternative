use crate::settings::{DecodeMode, VoiceWaveSettings};
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LatencyMetricRecord {
    pub session_id: u64,
    pub timestamp_utc_ms: u64,
    pub capture_ms: u64,
    pub release_to_transcribing_ms: u64,
    #[serde(default)]
    pub effective_release_watchdog_ms: u64,
    pub decode_ms: u64,
    pub post_ms: u64,
    pub insert_ms: u64,
    pub total_ms: u64,
    #[serde(default)]
    pub release_to_inserted_ms: u64,
    pub audio_duration_ms: u64,
    pub model_id: String,
    pub decode_mode: DecodeMode,
    pub watchdog_recovered: bool,
    pub segments_captured: u32,
    pub release_stop_detected_at_utc_ms: u64,
    pub model_init_ms: u64,
    pub audio_condition_ms: u64,
    pub decode_compute_ms: u64,
    pub runtime_cache_hit: bool,
    pub backend_requested: String,
    pub backend_used: String,
    pub backend_fallback: bool,
    pub hold_to_first_draft_ms: u64,
    pub incremental_decode_ms: u64,
    pub release_finalize_ms: u64,
    pub incremental_windows_decoded: u32,
    pub finalize_tail_audio_ms: u64,
    #[serde(default)]
    pub decode_policy_mode_selected: Option<DecodeMode>,
    #[serde(default)]
    pub decode_policy_reason: Option<String>,
    #[serde(default)]
    pub asr_integrity_percent: f32,
    #[serde(default)]
    pub asr_raw_word_count: u32,
    #[serde(default)]
    pub asr_final_word_count: u32,
    #[serde(default)]
    pub fw_low_coherence: bool,
    #[serde(default)]
    pub fw_retry_used: bool,
    #[serde(default)]
    pub fw_literal_retry_used: bool,
    #[serde(default)]
    pub audio_pipeline_version: String,
    #[serde(default)]
    pub fw_avg_logprob: Option<f32>,
    #[serde(default)]
    pub fw_no_speech_prob: Option<f32>,
    #[serde(default)]
    pub fw_compression_ratio: Option<f32>,
    #[serde(default)]
    pub fw_shadow_candidate_version: Option<String>,
    #[serde(default)]
    pub fw_shadow_quality_delta: Option<f32>,
    #[serde(default)]
    pub fw_shadow_candidate_avg_logprob: Option<f32>,
    #[serde(default)]
    pub fw_shadow_candidate_no_speech_prob: Option<f32>,
    #[serde(default)]
    pub fw_shadow_candidate_retry_used: Option<bool>,
    #[serde(default)]
    pub fw_shadow_candidate_low_coherence: Option<bool>,
    #[serde(default)]
    pub fw_shadow_candidate_decode_compute_ms: Option<u64>,
    #[serde(default)]
    pub fw_shadow_candidate_won: Option<bool>,
    #[serde(default)]
    pub audio_pipeline_fallback_engaged: bool,
    #[serde(default)]
    pub audio_pipeline_fallback_remaining: u32,
    #[serde(default)]
    pub warm_start_hit: bool,
    #[serde(default)]
    pub worker_reused: bool,
    #[serde(default)]
    pub correction_candidates_count: u32,
    #[serde(default)]
    pub insertion_method: Option<String>,
    #[serde(default)]
    pub insertion_target_class: Option<String>,
    pub success: bool,
    /// OS input volume of the default capture endpoint at dictation start
    /// (0-100). Ground truth for "quality suddenly dropped" triage: external
    /// apps lower this without user action.
    #[serde(default)]
    pub mic_input_volume_percent: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelReliabilitySummary {
    pub model_id: String,
    pub sample_count: usize,
    pub success_rate_percent: f32,
    pub watchdog_recovery_rate_percent: f32,
    pub p95_release_to_final_ms: u64,
    pub p95_release_to_transcribing_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsStatus {
    pub opt_in: bool,
    pub record_count: usize,
    pub last_export_path: Option<String>,
    pub last_exported_at_utc_ms: Option<u64>,
    pub watchdog_recovery_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsExportResult {
    pub file_path: String,
    pub exported_at_utc_ms: u64,
    pub record_count: usize,
    pub redaction_summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct DiagnosticsStore {
    records: Vec<LatencyMetricRecord>,
    last_export_path: Option<String>,
    last_exported_at_utc_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EncryptedDiagnosticsStore {
    version: u8,
    nonce_b64: String,
    ciphertext_b64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DiagnosticsBundle {
    version: u8,
    app_version: String,
    exported_at_utc_ms: u64,
    redaction_summary: String,
    settings_snapshot: DiagnosticsSettingsSnapshot,
    aggregate: DiagnosticsAggregate,
    recent_records: Vec<LatencyMetricRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DiagnosticsSettingsSnapshot {
    active_model: String,
    decode_mode: DecodeMode,
    vad_threshold: f32,
    max_utterance_ms: u64,
    release_tail_ms: u64,
    prefer_clipboard_fallback: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DiagnosticsAggregate {
    total_sessions: usize,
    successful_sessions: usize,
    success_rate_percent: f32,
    watchdog_recovery_count: u64,
    p50_total_ms: u64,
    p95_total_ms: u64,
    p50_release_to_transcribing_ms: u64,
    p95_release_to_transcribing_ms: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum DiagnosticsError {
    #[error("failed to read diagnostics store: {0}")]
    Read(std::io::Error),
    #[error("failed to write diagnostics store: {0}")]
    Write(std::io::Error),
    #[error("failed to parse diagnostics JSON: {0}")]
    Parse(serde_json::Error),
    #[error("failed to encrypt diagnostics store: {0}")]
    Encrypt(String),
    #[error("failed to decrypt diagnostics store: {0}")]
    Decrypt(String),
    #[error("failed to decode diagnostics key: {0}")]
    KeyDecode(String),
    #[error("cannot resolve app data directory")]
    AppData,
    #[error("diagnostics export requires opt-in to be enabled")]
    OptInRequired,
}

pub struct DiagnosticsManager {
    store_path: PathBuf,
    _key_path: PathBuf,
    key: [u8; 32],
    export_dir: PathBuf,
    store: DiagnosticsStore,
}

impl DiagnosticsManager {
    const MAX_RECORDS: usize = 5_000;
    const EXPORT_RECENT_LIMIT: usize = 500;

    pub fn new() -> Result<Self, DiagnosticsError> {
        let proj_dirs =
            ProjectDirs::from("com", "voicewave", "localcore").ok_or(DiagnosticsError::AppData)?;
        let store_path = proj_dirs.config_dir().join("diagnostics.json");
        let key_path = proj_dirs.config_dir().join("diagnostics.key");
        let export_dir = proj_dirs.data_dir().join("diagnostics-exports");
        Self::from_paths(store_path, key_path, export_dir)
    }

    pub fn from_paths(
        store_path: impl AsRef<Path>,
        key_path: impl AsRef<Path>,
        export_dir: impl AsRef<Path>,
    ) -> Result<Self, DiagnosticsError> {
        let store_path = store_path.as_ref().to_path_buf();
        let key_path = key_path.as_ref().to_path_buf();
        let key = load_or_create_key(&key_path)?;
        let export_dir = export_dir.as_ref().to_path_buf();
        let mut manager = Self {
            store_path,
            _key_path: key_path,
            key,
            export_dir,
            store: DiagnosticsStore::default(),
        };
        manager.load()?;
        Ok(manager)
    }

    /// Read access to the raw per-utterance records (used by the stats
    /// module's one-time backfill).
    pub fn latency_records(&self) -> &[LatencyMetricRecord] {
        &self.store.records
    }

    pub fn record_latency(&mut self, record: LatencyMetricRecord) -> Result<(), DiagnosticsError> {
        self.store.records.push(record);
        if self.store.records.len() > Self::MAX_RECORDS {
            let drop_count = self.store.records.len() - Self::MAX_RECORDS;
            self.store.records.drain(0..drop_count);
        }
        self.persist()
    }

    pub fn status(&self, opt_in: bool, watchdog_recovery_count: u64) -> DiagnosticsStatus {
        DiagnosticsStatus {
            opt_in,
            record_count: self.store.records.len(),
            last_export_path: self.store.last_export_path.clone(),
            last_exported_at_utc_ms: self.store.last_exported_at_utc_ms,
            watchdog_recovery_count,
        }
    }

    pub fn summarize_model_reliability(
        &self,
        model_id: &str,
        max_records: usize,
    ) -> Option<ModelReliabilitySummary> {
        let keep = max_records.max(1);
        let mut rows = self
            .store
            .records
            .iter()
            .filter(|row| row.model_id == model_id)
            .collect::<Vec<_>>();
        if rows.is_empty() {
            return None;
        }
        rows.sort_by_key(|row| row.timestamp_utc_ms);
        if rows.len() > keep {
            rows = rows[rows.len() - keep..].to_vec();
        }

        let sample_count = rows.len();
        let success_count = rows.iter().filter(|row| row.success).count();
        let watchdog_count = rows.iter().filter(|row| row.watchdog_recovered).count();
        let success_rate_percent = (success_count as f32 / sample_count as f32) * 100.0;
        let watchdog_recovery_rate_percent = (watchdog_count as f32 / sample_count as f32) * 100.0;
        let mut release_to_final_values = rows
            .iter()
            .map(|row| {
                row.release_to_transcribing_ms
                    .saturating_add(row.decode_ms)
                    .saturating_add(row.post_ms)
            })
            .collect::<Vec<_>>();
        release_to_final_values.sort_unstable();
        let mut release_to_transcribing_values = rows
            .iter()
            .map(|row| row.release_to_transcribing_ms)
            .collect::<Vec<_>>();
        release_to_transcribing_values.sort_unstable();

        Some(ModelReliabilitySummary {
            model_id: model_id.to_string(),
            sample_count,
            success_rate_percent,
            watchdog_recovery_rate_percent,
            p95_release_to_final_ms: percentile_u64(&release_to_final_values, 0.95),
            p95_release_to_transcribing_ms: percentile_u64(&release_to_transcribing_values, 0.95),
        })
    }

    pub fn export_bundle(
        &mut self,
        opt_in: bool,
        app_version: &str,
        settings: &VoiceWaveSettings,
        watchdog_recovery_count: u64,
    ) -> Result<DiagnosticsExportResult, DiagnosticsError> {
        if !opt_in {
            return Err(DiagnosticsError::OptInRequired);
        }

        fs::create_dir_all(&self.export_dir).map_err(DiagnosticsError::Write)?;
        let exported_at = now_utc_ms();
        let filename = format!("voicewave-diagnostics-{exported_at}.json");
        let file_path = self.export_dir.join(filename);

        let redaction_summary =
            "Export contains no raw audio and no transcript text; only redacted performance metadata."
                .to_string();
        let bundle = DiagnosticsBundle {
            version: 1,
            app_version: app_version.to_string(),
            exported_at_utc_ms: exported_at,
            redaction_summary: redaction_summary.clone(),
            settings_snapshot: DiagnosticsSettingsSnapshot {
                active_model: settings.active_model.clone(),
                decode_mode: settings.decode_mode,
                vad_threshold: settings.vad_threshold,
                max_utterance_ms: settings.max_utterance_ms,
                release_tail_ms: settings.release_tail_ms,
                prefer_clipboard_fallback: settings.prefer_clipboard_fallback,
            },
            aggregate: aggregate_metrics(&self.store.records, watchdog_recovery_count),
            recent_records: self
                .store
                .records
                .iter()
                .rev()
                .take(Self::EXPORT_RECENT_LIMIT)
                .cloned()
                .collect(),
        };
        let encoded = serde_json::to_string_pretty(&bundle).map_err(DiagnosticsError::Parse)?;
        fs::write(&file_path, encoded).map_err(DiagnosticsError::Write)?;

        self.store.last_export_path = Some(file_path.to_string_lossy().to_string());
        self.store.last_exported_at_utc_ms = Some(exported_at);
        self.persist()?;

        Ok(DiagnosticsExportResult {
            file_path: file_path.to_string_lossy().to_string(),
            exported_at_utc_ms: exported_at,
            record_count: self.store.records.len(),
            redaction_summary,
        })
    }

    fn load(&mut self) -> Result<(), DiagnosticsError> {
        if !self.store_path.exists() {
            return Ok(());
        }
        let raw = fs::read_to_string(&self.store_path).map_err(DiagnosticsError::Read)?;
        if let Ok(encrypted) = serde_json::from_str::<EncryptedDiagnosticsStore>(&raw) {
            self.store = decrypt_diagnostics_store(&encrypted, &self.key)?;
            return Ok(());
        }

        self.store = serde_json::from_str(&raw).map_err(DiagnosticsError::Parse)?;
        backup_legacy_plaintext(&self.store_path)?;
        self.persist()?;
        Ok(())
    }

    fn persist(&self) -> Result<(), DiagnosticsError> {
        if let Some(parent) = self.store_path.parent() {
            fs::create_dir_all(parent).map_err(DiagnosticsError::Write)?;
        }
        let encrypted = encrypt_diagnostics_store(&self.store, &self.key)?;
        let raw = serde_json::to_string_pretty(&encrypted).map_err(DiagnosticsError::Parse)?;
        fs::write(&self.store_path, raw).map_err(DiagnosticsError::Write)?;
        Ok(())
    }
}

fn backup_legacy_plaintext(path: &Path) -> Result<(), DiagnosticsError> {
    if !path.exists() {
        return Ok(());
    }
    let backup_path = path.with_extension("json.bak");
    if backup_path.exists() {
        return Ok(());
    }
    fs::copy(path, backup_path).map_err(DiagnosticsError::Write)?;
    Ok(())
}

fn load_or_create_key(path: &PathBuf) -> Result<[u8; 32], DiagnosticsError> {
    if path.exists() {
        let encoded = fs::read_to_string(path).map_err(DiagnosticsError::Read)?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded.trim())
            .map_err(|err| DiagnosticsError::KeyDecode(err.to_string()))?;
        if bytes.len() != 32 {
            return Err(DiagnosticsError::KeyDecode(
                "diagnostics.key must decode to 32 bytes".to_string(),
            ));
        }
        let mut key = [0_u8; 32];
        key.copy_from_slice(&bytes);
        return Ok(key);
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(DiagnosticsError::Write)?;
    }
    let mut key = [0_u8; 32];
    OsRng.fill_bytes(&mut key);
    let encoded = base64::engine::general_purpose::STANDARD.encode(key);
    fs::write(path, encoded).map_err(DiagnosticsError::Write)?;
    Ok(key)
}

fn encrypt_diagnostics_store(
    store: &DiagnosticsStore,
    key: &[u8; 32],
) -> Result<EncryptedDiagnosticsStore, DiagnosticsError> {
    let cipher =
        Aes256Gcm::new_from_slice(key).map_err(|err| DiagnosticsError::Encrypt(err.to_string()))?;
    let mut nonce_bytes = [0_u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let plaintext = serde_json::to_vec(store).map_err(DiagnosticsError::Parse)?;
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_ref())
        .map_err(|err| DiagnosticsError::Encrypt(err.to_string()))?;

    Ok(EncryptedDiagnosticsStore {
        version: 1,
        nonce_b64: base64::engine::general_purpose::STANDARD.encode(nonce_bytes),
        ciphertext_b64: base64::engine::general_purpose::STANDARD.encode(ciphertext),
    })
}

fn decrypt_diagnostics_store(
    encrypted: &EncryptedDiagnosticsStore,
    key: &[u8; 32],
) -> Result<DiagnosticsStore, DiagnosticsError> {
    if encrypted.version != 1 {
        return Err(DiagnosticsError::Decrypt(format!(
            "unsupported diagnostics encryption version {}",
            encrypted.version
        )));
    }

    let nonce_bytes = base64::engine::general_purpose::STANDARD
        .decode(encrypted.nonce_b64.as_bytes())
        .map_err(|err| DiagnosticsError::Decrypt(err.to_string()))?;
    let ciphertext = base64::engine::general_purpose::STANDARD
        .decode(encrypted.ciphertext_b64.as_bytes())
        .map_err(|err| DiagnosticsError::Decrypt(err.to_string()))?;
    if nonce_bytes.len() != 12 {
        return Err(DiagnosticsError::Decrypt(
            "diagnostics nonce must be 12 bytes".to_string(),
        ));
    }

    let cipher =
        Aes256Gcm::new_from_slice(key).map_err(|err| DiagnosticsError::Decrypt(err.to_string()))?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext.as_ref())
        .map_err(|err| DiagnosticsError::Decrypt(err.to_string()))?;
    serde_json::from_slice(&plaintext).map_err(DiagnosticsError::Parse)
}

fn aggregate_metrics(
    records: &[LatencyMetricRecord],
    watchdog_recovery_count: u64,
) -> DiagnosticsAggregate {
    let mut total_values = records.iter().map(|row| row.total_ms).collect::<Vec<_>>();
    total_values.sort_unstable();
    let mut release_values = records
        .iter()
        .map(|row| row.release_to_transcribing_ms)
        .collect::<Vec<_>>();
    release_values.sort_unstable();

    let successful_sessions = records.iter().filter(|row| row.success).count();
    let success_rate_percent = if records.is_empty() {
        0.0
    } else {
        (successful_sessions as f32 / records.len() as f32) * 100.0
    };
    let p50_total_ms = percentile_u64(&total_values, 0.50);
    let p95_total_ms = percentile_u64(&total_values, 0.95);
    let p50_release_to_transcribing_ms = percentile_u64(&release_values, 0.50);
    let p95_release_to_transcribing_ms = percentile_u64(&release_values, 0.95);

    DiagnosticsAggregate {
        total_sessions: records.len(),
        successful_sessions,
        success_rate_percent,
        watchdog_recovery_count,
        p50_total_ms,
        p95_total_ms,
        p50_release_to_transcribing_ms,
        p95_release_to_transcribing_ms,
    }
}

fn percentile_u64(values: &[u64], percentile: f32) -> u64 {
    if values.is_empty() {
        return 0;
    }
    let clamped = percentile.clamp(0.0, 1.0);
    let idx = ((values.len() - 1) as f32 * clamped).round() as usize;
    values[idx]
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
    use crate::settings::VoiceWaveSettings;

    fn test_manager(name: &str) -> DiagnosticsManager {
        let base = std::env::temp_dir().join(format!("voicewave-diagnostics-{name}-{}", now_utc_ms()));
        let store_path = base.with_extension("json");
        let key_path = base.with_extension("key");
        let export_dir = std::env::temp_dir().join(format!("voicewave-diagnostics-export-{name}"));
        let key = load_or_create_key(&key_path).expect("key");
        DiagnosticsManager {
            store_path,
            _key_path: key_path,
            key,
            export_dir,
            store: DiagnosticsStore::default(),
        }
    }

    #[test]
    fn aggregate_handles_empty_input() {
        let aggregate = aggregate_metrics(&[], 0);
        assert_eq!(aggregate.total_sessions, 0);
        assert_eq!(aggregate.p95_total_ms, 0);
    }

    #[test]
    fn aggregate_reports_non_zero_percentiles() {
        let rows = vec![
            LatencyMetricRecord {
                session_id: 1,
                timestamp_utc_ms: 1,
                capture_ms: 10,
                release_to_transcribing_ms: 20,
                effective_release_watchdog_ms: 300,
                decode_ms: 30,
                post_ms: 10,
                insert_ms: 5,
                total_ms: 55,
                release_to_inserted_ms: 55,
                audio_duration_ms: 400,
                model_id: "tiny.en".to_string(),
                decode_mode: DecodeMode::Balanced,
                watchdog_recovered: false,
                segments_captured: 2,
                release_stop_detected_at_utc_ms: 1,
                model_init_ms: 18,
                audio_condition_ms: 7,
                decode_compute_ms: 24,
                runtime_cache_hit: false,
                backend_requested: "cpu".to_string(),
                backend_used: "cpu".to_string(),
                backend_fallback: false,
                hold_to_first_draft_ms: 0,
                incremental_decode_ms: 0,
                release_finalize_ms: 30,
                incremental_windows_decoded: 0,
                finalize_tail_audio_ms: 400,
                decode_policy_mode_selected: Some(DecodeMode::Balanced),
                decode_policy_reason: Some("runtime-policy:balanced".to_string()),
                asr_integrity_percent: 100.0,
                asr_raw_word_count: 8,
                asr_final_word_count: 8,
                fw_low_coherence: false,
                fw_retry_used: false,
                fw_literal_retry_used: false,
                audio_pipeline_version: "v1".to_string(),
                fw_avg_logprob: Some(-0.42),
                fw_no_speech_prob: Some(0.18),
                fw_compression_ratio: Some(1.3),
                fw_shadow_candidate_version: None,
                fw_shadow_quality_delta: None,
                fw_shadow_candidate_avg_logprob: None,
                fw_shadow_candidate_no_speech_prob: None,
                fw_shadow_candidate_retry_used: None,
                fw_shadow_candidate_low_coherence: None,
                fw_shadow_candidate_decode_compute_ms: None,
                fw_shadow_candidate_won: None,
                audio_pipeline_fallback_engaged: false,
                audio_pipeline_fallback_remaining: 0,
                warm_start_hit: false,
                worker_reused: false,
                correction_candidates_count: 0,
                insertion_method: Some("direct".to_string()),
                insertion_target_class: Some("editor".to_string()),
                success: true,
                mic_input_volume_percent: None,
            },
            LatencyMetricRecord {
                session_id: 2,
                timestamp_utc_ms: 2,
                capture_ms: 15,
                release_to_transcribing_ms: 40,
                effective_release_watchdog_ms: 300,
                decode_ms: 50,
                post_ms: 12,
                insert_ms: 6,
                total_ms: 83,
                release_to_inserted_ms: 83,
                audio_duration_ms: 620,
                model_id: "small.en".to_string(),
                decode_mode: DecodeMode::Fast,
                watchdog_recovered: true,
                segments_captured: 3,
                release_stop_detected_at_utc_ms: 2,
                model_init_ms: 0,
                audio_condition_ms: 5,
                decode_compute_ms: 38,
                runtime_cache_hit: true,
                backend_requested: "cuda".to_string(),
                backend_used: "cuda".to_string(),
                backend_fallback: false,
                hold_to_first_draft_ms: 850,
                incremental_decode_ms: 1100,
                release_finalize_ms: 900,
                incremental_windows_decoded: 2,
                finalize_tail_audio_ms: 620,
                decode_policy_mode_selected: Some(DecodeMode::Fast),
                decode_policy_reason: Some("runtime-policy:fast".to_string()),
                asr_integrity_percent: 95.0,
                asr_raw_word_count: 11,
                asr_final_word_count: 12,
                fw_low_coherence: false,
                fw_retry_used: true,
                fw_literal_retry_used: false,
                audio_pipeline_version: "v1".to_string(),
                fw_avg_logprob: Some(-0.78),
                fw_no_speech_prob: Some(0.33),
                fw_compression_ratio: Some(1.8),
                fw_shadow_candidate_version: None,
                fw_shadow_quality_delta: None,
                fw_shadow_candidate_avg_logprob: None,
                fw_shadow_candidate_no_speech_prob: None,
                fw_shadow_candidate_retry_used: None,
                fw_shadow_candidate_low_coherence: None,
                fw_shadow_candidate_decode_compute_ms: None,
                fw_shadow_candidate_won: None,
                audio_pipeline_fallback_engaged: false,
                audio_pipeline_fallback_remaining: 0,
                warm_start_hit: true,
                worker_reused: true,
                correction_candidates_count: 1,
                insertion_method: Some("clipboardPaste".to_string()),
                insertion_target_class: Some("browser".to_string()),
                success: true,
                mic_input_volume_percent: None,
            },
        ];

        let aggregate = aggregate_metrics(&rows, 1);
        assert_eq!(aggregate.total_sessions, 2);
        assert!(aggregate.p95_total_ms >= aggregate.p50_total_ms);
        assert!(
            aggregate.p95_release_to_transcribing_ms >= aggregate.p50_release_to_transcribing_ms
        );
    }

    #[test]
    fn export_requires_opt_in() {
        let mut manager = test_manager("opt-in");

        let err = manager
            .export_bundle(false, "0.1.0", &VoiceWaveSettings::default(), 0)
            .expect_err("export should require opt in");
        assert!(matches!(err, DiagnosticsError::OptInRequired));
    }

    #[test]
    fn model_reliability_summary_uses_recent_window() {
        let mut manager = test_manager("summary");
        manager.store.records = vec![
            LatencyMetricRecord {
                session_id: 1,
                timestamp_utc_ms: 10,
                capture_ms: 5,
                release_to_transcribing_ms: 20,
                effective_release_watchdog_ms: 300,
                decode_ms: 100,
                post_ms: 10,
                insert_ms: 4,
                total_ms: 134,
                release_to_inserted_ms: 134,
                audio_duration_ms: 800,
                model_id: "small.en".to_string(),
                decode_mode: DecodeMode::Balanced,
                watchdog_recovered: false,
                segments_captured: 3,
                release_stop_detected_at_utc_ms: 10,
                model_init_ms: 0,
                audio_condition_ms: 0,
                decode_compute_ms: 0,
                runtime_cache_hit: true,
                backend_requested: "cpu".to_string(),
                backend_used: "cpu".to_string(),
                backend_fallback: false,
                hold_to_first_draft_ms: 0,
                incremental_decode_ms: 0,
                release_finalize_ms: 0,
                incremental_windows_decoded: 0,
                finalize_tail_audio_ms: 0,
                decode_policy_mode_selected: Some(DecodeMode::Balanced),
                decode_policy_reason: Some("runtime-policy:balanced".to_string()),
                asr_integrity_percent: 72.0,
                asr_raw_word_count: 4,
                asr_final_word_count: 7,
                fw_low_coherence: true,
                fw_retry_used: true,
                fw_literal_retry_used: false,
                audio_pipeline_version: "v1".to_string(),
                fw_avg_logprob: Some(-0.92),
                fw_no_speech_prob: Some(0.44),
                fw_compression_ratio: Some(2.0),
                fw_shadow_candidate_version: None,
                fw_shadow_quality_delta: None,
                fw_shadow_candidate_avg_logprob: None,
                fw_shadow_candidate_no_speech_prob: None,
                fw_shadow_candidate_retry_used: None,
                fw_shadow_candidate_low_coherence: None,
                fw_shadow_candidate_decode_compute_ms: None,
                fw_shadow_candidate_won: None,
                audio_pipeline_fallback_engaged: false,
                audio_pipeline_fallback_remaining: 0,
                warm_start_hit: true,
                worker_reused: true,
                correction_candidates_count: 2,
                insertion_method: Some("historyFallback".to_string()),
                insertion_target_class: Some("unknown".to_string()),
                success: false,
                mic_input_volume_percent: None,
            },
            LatencyMetricRecord {
                session_id: 2,
                timestamp_utc_ms: 20,
                capture_ms: 5,
                release_to_transcribing_ms: 15,
                effective_release_watchdog_ms: 300,
                decode_ms: 80,
                post_ms: 10,
                insert_ms: 4,
                total_ms: 109,
                release_to_inserted_ms: 109,
                audio_duration_ms: 700,
                model_id: "small.en".to_string(),
                decode_mode: DecodeMode::Balanced,
                watchdog_recovered: false,
                segments_captured: 3,
                release_stop_detected_at_utc_ms: 20,
                model_init_ms: 0,
                audio_condition_ms: 0,
                decode_compute_ms: 0,
                runtime_cache_hit: true,
                backend_requested: "cpu".to_string(),
                backend_used: "cpu".to_string(),
                backend_fallback: false,
                hold_to_first_draft_ms: 0,
                incremental_decode_ms: 0,
                release_finalize_ms: 0,
                incremental_windows_decoded: 0,
                finalize_tail_audio_ms: 0,
                decode_policy_mode_selected: Some(DecodeMode::Balanced),
                decode_policy_reason: Some("runtime-policy:balanced".to_string()),
                asr_integrity_percent: 100.0,
                asr_raw_word_count: 8,
                asr_final_word_count: 8,
                fw_low_coherence: false,
                fw_retry_used: false,
                fw_literal_retry_used: false,
                audio_pipeline_version: "v1".to_string(),
                fw_avg_logprob: Some(-0.40),
                fw_no_speech_prob: Some(0.2),
                fw_compression_ratio: Some(1.2),
                fw_shadow_candidate_version: None,
                fw_shadow_quality_delta: None,
                fw_shadow_candidate_avg_logprob: None,
                fw_shadow_candidate_no_speech_prob: None,
                fw_shadow_candidate_retry_used: None,
                fw_shadow_candidate_low_coherence: None,
                fw_shadow_candidate_decode_compute_ms: None,
                fw_shadow_candidate_won: None,
                audio_pipeline_fallback_engaged: false,
                audio_pipeline_fallback_remaining: 0,
                warm_start_hit: true,
                worker_reused: true,
                correction_candidates_count: 0,
                insertion_method: Some("direct".to_string()),
                insertion_target_class: Some("editor".to_string()),
                success: true,
                mic_input_volume_percent: None,
            },
        ];

        let summary = manager
            .summarize_model_reliability("small.en", 1)
            .expect("summary should exist");
        assert_eq!(summary.sample_count, 1);
        assert_eq!(summary.success_rate_percent.round() as u32, 100);
        assert_eq!(summary.p95_release_to_transcribing_ms, 15);
    }

    #[test]
    fn persisted_diagnostics_is_encrypted() {
        let mut manager = test_manager("encrypt");
        manager
            .record_latency(LatencyMetricRecord {
                session_id: 3,
                timestamp_utc_ms: now_utc_ms(),
                capture_ms: 8,
                release_to_transcribing_ms: 12,
                effective_release_watchdog_ms: 300,
                decode_ms: 22,
                post_ms: 4,
                insert_ms: 3,
                total_ms: 41,
                release_to_inserted_ms: 41,
                audio_duration_ms: 320,
                model_id: "small.en".to_string(),
                decode_mode: DecodeMode::Balanced,
                watchdog_recovered: false,
                segments_captured: 1,
                release_stop_detected_at_utc_ms: now_utc_ms(),
                model_init_ms: 0,
                audio_condition_ms: 0,
                decode_compute_ms: 0,
                runtime_cache_hit: true,
                backend_requested: "cpu".to_string(),
                backend_used: "cpu".to_string(),
                backend_fallback: false,
                hold_to_first_draft_ms: 0,
                incremental_decode_ms: 0,
                release_finalize_ms: 0,
                incremental_windows_decoded: 0,
                finalize_tail_audio_ms: 0,
                decode_policy_mode_selected: Some(DecodeMode::Balanced),
                decode_policy_reason: Some("runtime-policy:balanced".to_string()),
                asr_integrity_percent: 100.0,
                asr_raw_word_count: 6,
                asr_final_word_count: 6,
                fw_low_coherence: false,
                fw_retry_used: false,
                fw_literal_retry_used: false,
                audio_pipeline_version: "v1".to_string(),
                fw_avg_logprob: Some(-0.3),
                fw_no_speech_prob: Some(0.1),
                fw_compression_ratio: Some(1.1),
                fw_shadow_candidate_version: None,
                fw_shadow_quality_delta: None,
                fw_shadow_candidate_avg_logprob: None,
                fw_shadow_candidate_no_speech_prob: None,
                fw_shadow_candidate_retry_used: None,
                fw_shadow_candidate_low_coherence: None,
                fw_shadow_candidate_decode_compute_ms: None,
                fw_shadow_candidate_won: None,
                audio_pipeline_fallback_engaged: false,
                audio_pipeline_fallback_remaining: 0,
                warm_start_hit: false,
                worker_reused: false,
                correction_candidates_count: 0,
                insertion_method: Some("direct".to_string()),
                insertion_target_class: Some("editor".to_string()),
                success: true,
                mic_input_volume_percent: None,
            })
            .expect("persist encrypted diagnostics");

        let raw = fs::read_to_string(&manager.store_path).expect("read diagnostics");
        assert!(!raw.contains("\"records\""));
        assert!(raw.contains("ciphertextB64"));
    }
}
