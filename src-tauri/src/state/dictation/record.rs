//! Recording: latency telemetry (UI event + opt-in diagnostics), the
//! always-on Stats numbers, the analytics day accumulator, and the background
//! History rescue write and dictionary-candidate queue.

use super::capture::CapturedAudio;
use super::insert::Insertion;
use super::polish::Delivery;
use super::post_process::FormattedText;
use super::transcribe::{DecodeModeChoice, Decoded, TranscribingStart, Transcription};
use super::DictationRun;
use crate::analytics::{self, DictationFact, FallbackReason, Finished, InsertKind};
use crate::diagnostics::LatencyMetricRecord;
use crate::history::PolishHistoryMeta;
use crate::settings::DecodeMode;
use crate::state::{
    now_utc_ms, sample_count_to_ms, DictationMode, VoiceWaveController, MAIN_WINDOW_LABEL,
    PILL_WINDOW_LABEL,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::Emitter;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LatencyBreakdownEvent {
    session_id: u64,
    capture_ms: u64,
    release_to_transcribing_ms: u64,
    effective_release_watchdog_ms: u64,
    watchdog_recovered: bool,
    segments_captured: u32,
    release_stop_detected_at_utc_ms: u64,
    model_init_ms: u64,
    audio_condition_ms: u64,
    decode_compute_ms: u64,
    runtime_cache_hit: bool,
    backend_requested: String,
    backend_used: String,
    backend_fallback: bool,
    asr_engine: String,
    asr_step_down_reason: Option<String>,
    hold_to_first_draft_ms: u64,
    incremental_decode_ms: u64,
    release_finalize_ms: u64,
    incremental_windows_decoded: u32,
    finalize_tail_audio_ms: u64,
    asr_integrity_percent: f32,
    asr_raw_word_count: u32,
    asr_final_word_count: u32,
    decode_ms: u64,
    post_ms: u64,
    insert_ms: u64,
    total_ms: u64,
    release_to_inserted_ms: u64,
    audio_duration_ms: u64,
    model_id: String,
    decode_mode: DecodeMode,
    decode_policy_mode_selected: DecodeMode,
    decode_policy_reason: String,
    fw_low_coherence: bool,
    fw_retry_used: bool,
    fw_literal_retry_used: bool,
    audio_pipeline_version: String,
    fw_avg_logprob: Option<f32>,
    fw_no_speech_prob: Option<f32>,
    fw_compression_ratio: Option<f32>,
    fw_shadow_candidate_version: Option<String>,
    fw_shadow_quality_delta: Option<f32>,
    fw_shadow_candidate_avg_logprob: Option<f32>,
    fw_shadow_candidate_no_speech_prob: Option<f32>,
    fw_shadow_candidate_retry_used: Option<bool>,
    fw_shadow_candidate_low_coherence: Option<bool>,
    fw_shadow_candidate_decode_compute_ms: Option<u64>,
    fw_shadow_candidate_won: Option<bool>,
    audio_pipeline_fallback_engaged: bool,
    audio_pipeline_fallback_remaining: u32,
    warm_start_hit: bool,
    worker_reused: bool,
    correction_candidates_count: u32,
    insertion_method: String,
    insertion_target_class: String,
    mic_input_volume_percent: Option<u8>,
    /// A microphone dictation whose text landed (clipboard-only counts;
    /// fixture checks never do). Drives the usage and first-success
    /// telemetry in the frontend.
    dictation_landed: bool,
}

/// The UI event carries the record's measurements in its own shape: no
/// timestamp, and plain values where the record keeps an `Option` (always
/// `Some` for a dictation; `asr_engine` is `None` only when empty).
impl LatencyBreakdownEvent {
    fn new(record: &LatencyMetricRecord, mode: DictationMode) -> Self {
        Self {
            session_id: record.session_id,
            capture_ms: record.capture_ms,
            release_to_transcribing_ms: record.release_to_transcribing_ms,
            effective_release_watchdog_ms: record.effective_release_watchdog_ms,
            watchdog_recovered: record.watchdog_recovered,
            segments_captured: record.segments_captured,
            release_stop_detected_at_utc_ms: record.release_stop_detected_at_utc_ms,
            model_init_ms: record.model_init_ms,
            audio_condition_ms: record.audio_condition_ms,
            decode_compute_ms: record.decode_compute_ms,
            runtime_cache_hit: record.runtime_cache_hit,
            backend_requested: record.backend_requested.clone(),
            backend_used: record.backend_used.clone(),
            backend_fallback: record.backend_fallback,
            asr_engine: record.asr_engine.clone().unwrap_or_default(),
            asr_step_down_reason: record.asr_step_down_reason.clone(),
            hold_to_first_draft_ms: record.hold_to_first_draft_ms,
            incremental_decode_ms: record.incremental_decode_ms,
            release_finalize_ms: record.release_finalize_ms,
            incremental_windows_decoded: record.incremental_windows_decoded,
            finalize_tail_audio_ms: record.finalize_tail_audio_ms,
            asr_integrity_percent: record.asr_integrity_percent,
            asr_raw_word_count: record.asr_raw_word_count,
            asr_final_word_count: record.asr_final_word_count,
            decode_ms: record.decode_ms,
            post_ms: record.post_ms,
            insert_ms: record.insert_ms,
            total_ms: record.total_ms,
            release_to_inserted_ms: record.release_to_inserted_ms,
            audio_duration_ms: record.audio_duration_ms,
            model_id: record.model_id.clone(),
            decode_mode: record.decode_mode,
            decode_policy_mode_selected: record.decode_policy_mode_selected.unwrap_or_default(),
            decode_policy_reason: record.decode_policy_reason.clone().unwrap_or_default(),
            fw_low_coherence: record.fw_low_coherence,
            fw_retry_used: record.fw_retry_used,
            fw_literal_retry_used: record.fw_literal_retry_used,
            audio_pipeline_version: record.audio_pipeline_version.clone(),
            fw_avg_logprob: record.fw_avg_logprob,
            fw_no_speech_prob: record.fw_no_speech_prob,
            fw_compression_ratio: record.fw_compression_ratio,
            fw_shadow_candidate_version: record.fw_shadow_candidate_version.clone(),
            fw_shadow_quality_delta: record.fw_shadow_quality_delta,
            fw_shadow_candidate_avg_logprob: record.fw_shadow_candidate_avg_logprob,
            fw_shadow_candidate_no_speech_prob: record.fw_shadow_candidate_no_speech_prob,
            fw_shadow_candidate_retry_used: record.fw_shadow_candidate_retry_used,
            fw_shadow_candidate_low_coherence: record.fw_shadow_candidate_low_coherence,
            fw_shadow_candidate_decode_compute_ms: record.fw_shadow_candidate_decode_compute_ms,
            fw_shadow_candidate_won: record.fw_shadow_candidate_won,
            audio_pipeline_fallback_engaged: record.audio_pipeline_fallback_engaged,
            audio_pipeline_fallback_remaining: record.audio_pipeline_fallback_remaining,
            warm_start_hit: record.warm_start_hit,
            worker_reused: record.worker_reused,
            correction_candidates_count: record.correction_candidates_count,
            insertion_method: record.insertion_method.clone().unwrap_or_default(),
            insertion_target_class: record.insertion_target_class.clone().unwrap_or_default(),
            mic_input_volume_percent: record.mic_input_volume_percent,
            dictation_landed: record.success && mode == DictationMode::Microphone,
        }
    }
}

impl VoiceWaveController {
    /// Runs after the session is marked Inserted, for successful and failed
    /// insertions alike.
    pub(super) async fn record_dictation(
        &self,
        run: &DictationRun,
        capture: &CapturedAudio,
        transcription: Transcription,
        text: &FormattedText,
        delivery: Delivery,
        insertion: Insertion,
    ) {
        let DictationRun {
            ref app,
            mode,
            session_id,
            flow_started,
            ref settings,
            ..
        } = *run;
        let CapturedAudio {
            segment_count,
            ref source_samples,
            capture_ms,
            ref mic_input_volume_slot,
            ..
        } = *capture;
        let Transcription {
            worker: final_worker,
            mode_choice:
                DecodeModeChoice {
                    policy_selected: policy_selected_mode,
                    reason: decode_policy_reason,
                    ..
                },
            start:
                TranscribingStart {
                    release_to_transcribing_ms,
                    effective_release_watchdog_ms,
                    watchdog_recovered,
                    ..
                },
            decoded:
                Decoded {
                    telemetry: decode_telemetry,
                    release_finalize_ms,
                    decode_ms,
                    finalize_tail_audio_ms,
                    ..
                },
        } = transcription;
        let FormattedText {
            asr_integrity_percent,
            asr_raw_word_count,
            asr_final_word_count,
            ..
        } = *text;
        let Delivery {
            inserted_text,
            polish_meta,
            polish_wait_ms,
            ..
        } = delivery;
        let Insertion {
            success: insertion_success,
            method: insertion_method,
            target_class: insertion_target_class,
            history_recorded: insertion_history_recorded,
            blocked_by_elevation: insertion_blocked_by_elevation,
            app_kind: insertion_app_kind,
            correction_candidates,
            post_started,
            post_before_insert_ms,
            insert_ms,
            ..
        } = insertion;

        let post_after_insert_ms = post_started.elapsed().as_millis() as u64;
        let post_after_only_ms = post_after_insert_ms
            .saturating_sub(post_before_insert_ms)
            .saturating_sub(insert_ms);
        let post_ms = post_before_insert_ms.saturating_add(post_after_only_ms);
        let total_ms = flow_started.elapsed().as_millis() as u64;
        let release_to_final_ms = release_to_transcribing_ms
            .saturating_add(decode_ms)
            .saturating_add(post_ms);
        // The wait-validated polish pause (plan 010) counts toward the
        // user-perceived release-to-insert number, but deliberately NOT
        // toward release_to_final_ms: the decode policy learns from the
        // deterministic pipeline's speed and must not be punished for an
        // opt-in LLM wait.
        let release_to_inserted_ms = release_to_final_ms
            .saturating_add(polish_wait_ms)
            .saturating_add(insert_ms);
        if mode == DictationMode::Microphone {
            self.decode_policy
                .lock()
                .await
                .record_success(release_to_final_ms);
        }
        let audio_duration_ms =
            sample_count_to_ms(source_samples.len(), self.audio.target_sample_rate);
        let release_stop_detected_at_utc_ms = self
            .session_release_stop_detected_at_utc_ms(session_id)
            .await
            .unwrap_or(0);
        // Measured once: the persisted diagnostics record, and the UI latency
        // event derived from it.
        let latency = LatencyMetricRecord {
            session_id,
            timestamp_utc_ms: now_utc_ms(),
            capture_ms,
            release_to_transcribing_ms,
            effective_release_watchdog_ms,
            watchdog_recovered,
            segments_captured: segment_count,
            release_stop_detected_at_utc_ms,
            model_init_ms: decode_telemetry.model_init_ms,
            audio_condition_ms: decode_telemetry.audio_condition_ms,
            decode_compute_ms: decode_telemetry.decode_compute_ms,
            runtime_cache_hit: decode_telemetry.runtime_cache_hit,
            backend_requested: decode_telemetry.backend_requested,
            backend_used: decode_telemetry.backend_used,
            backend_fallback: decode_telemetry.backend_fallback,
            asr_engine: Some(decode_telemetry.asr_engine).filter(|engine| !engine.is_empty()),
            asr_step_down_reason: decode_telemetry.asr_step_down_reason,
            hold_to_first_draft_ms: 0,
            incremental_decode_ms: 0,
            release_finalize_ms,
            incremental_windows_decoded: 0,
            finalize_tail_audio_ms,
            asr_integrity_percent,
            asr_raw_word_count,
            asr_final_word_count,
            decode_ms,
            post_ms,
            insert_ms,
            total_ms,
            release_to_inserted_ms,
            audio_duration_ms,
            model_id: final_worker.active_model().to_string(),
            decode_mode: final_worker.decode_mode(),
            decode_policy_mode_selected: Some(policy_selected_mode),
            decode_policy_reason: Some(decode_policy_reason),
            fw_low_coherence: decode_telemetry.fw_low_coherence,
            fw_retry_used: decode_telemetry.fw_retry_used,
            fw_literal_retry_used: decode_telemetry.fw_literal_retry_used,
            audio_pipeline_version: decode_telemetry.audio_pipeline_version,
            fw_avg_logprob: decode_telemetry.fw_avg_logprob,
            fw_no_speech_prob: decode_telemetry.fw_no_speech_prob,
            fw_compression_ratio: decode_telemetry.fw_compression_ratio,
            fw_shadow_candidate_version: decode_telemetry.fw_shadow_candidate_version,
            fw_shadow_quality_delta: decode_telemetry.fw_shadow_quality_delta,
            fw_shadow_candidate_avg_logprob: decode_telemetry.fw_shadow_candidate_avg_logprob,
            fw_shadow_candidate_no_speech_prob: decode_telemetry.fw_shadow_candidate_no_speech_prob,
            fw_shadow_candidate_retry_used: decode_telemetry.fw_shadow_candidate_retry_used,
            fw_shadow_candidate_low_coherence: decode_telemetry.fw_shadow_candidate_low_coherence,
            fw_shadow_candidate_decode_compute_ms: decode_telemetry
                .fw_shadow_candidate_decode_compute_ms,
            fw_shadow_candidate_won: decode_telemetry.fw_shadow_candidate_won,
            audio_pipeline_fallback_engaged: decode_telemetry.audio_pipeline_fallback_engaged,
            audio_pipeline_fallback_remaining: decode_telemetry.audio_pipeline_fallback_remaining,
            warm_start_hit: decode_telemetry.runtime_cache_hit,
            worker_reused: decode_telemetry.runtime_cache_hit,
            correction_candidates_count: correction_candidates.len() as u32,
            insertion_method: Some(insertion_method),
            insertion_target_class: Some(insertion_target_class),
            success: insertion_success,
            mic_input_volume_percent: mic_input_volume_slot.lock().ok().and_then(|slot| *slot),
        };
        let _ = app.emit("voicewave://latency", LatencyBreakdownEvent::new(&latency, mode));
        // Analytics' day accumulator (plan 024 4.2): counts and timings only,
        // handed to the recorder thread. Not gated on `diagnostics_opt_in`
        // below, which is for the encrypted diagnostics store; analytics has
        // its own consent gate.
        if mode == DictationMode::Microphone {
            analytics::note_dictation(DictationFact::Finished(Finished {
                success: insertion_success,
                latency_ms: release_to_inserted_ms,
                audio_ms: audio_duration_ms,
                words: asr_final_word_count,
                on_gpu: latency.backend_used != "cpu",
                insert: match latency.insertion_method.as_deref() {
                    Some("direct") => Some(InsertKind::Typed),
                    Some("clipboardPaste") => Some(InsertKind::Pasted),
                    Some("clipboardOnly") if insertion_blocked_by_elevation => {
                        Some(InsertKind::BlockedElevated)
                    }
                    Some("clipboardOnly") => Some(InsertKind::ClipboardOnly),
                    Some("historyFallback") => Some(InsertKind::HistoryOnly),
                    _ => None,
                },
                app_kind: insertion_app_kind,
                format_style: settings.effective_polish_profile(),
                fallback: latency
                    .asr_step_down_reason
                    .as_deref()
                    .and_then(|reason| FallbackReason::classify(reason, latency.backend_fallback)),
            }));
        }
        // Always-on aggregate stats (counts and durations only, never text)
        // feeding the Stats tab. Deliberately OUTSIDE the diagnostics opt-in:
        // diagnostics gates the detailed per-utterance records below. Only
        // successful insertions feed these numbers so a decode that never
        // landed does not inflate "words dictated / time saved".
        if insertion_success {
            if let Err(err) = self.stats_manager.lock().await.record_dictation(
                now_utc_ms(),
                asr_final_word_count,
                asr_raw_word_count,
                audio_duration_ms,
                latency.insertion_target_class.as_deref(),
                latency.fw_avg_logprob,
                latency.fw_no_speech_prob,
            ) {
                tracing::warn!("stats record failed: {err}");
            }
        }
        if settings.diagnostics_opt_in {
            let _ = self
                .diagnostics_manager
                .lock()
                .await
                .record_latency(latency);
        }

        // Keep non-critical persistence off the hot path so release->final latency stays low.
        let history_manager = Arc::clone(&self.history_manager);
        let dictionary_manager = Arc::clone(&self.dictionary_manager);
        let transcript_for_history = inserted_text.clone();
        let transcript_for_dictionary_preview = inserted_text;
        let correction_candidates_for_dictionary = correction_candidates;
        let app_for_history = app.clone();
        // Rescue records still carry the profile fields, minus insertedText
        // (nothing landed in the target app).
        let rescue_polish_meta = PolishHistoryMeta {
            inserted_text: None,
            ..polish_meta.clone()
        };
        tauri::async_runtime::spawn(async move {
            // Rescue write only: when insert_text errored before persisting,
            // keep the transcript anyway so a failed insertion never loses the
            // user's words. On the happy path insert_text already recorded and
            // emitted, and recording here again would double every dictation.
            if !insertion_history_recorded {
                match history_manager.lock().await.record_transcript_with_polish(
                    &transcript_for_history,
                    Some(&rescue_polish_meta),
                ) {
                    Ok(_record_id) => {
                        if let Err(err) = app_for_history.emit_to(
                            MAIN_WINDOW_LABEL,
                            "voicewave://history-updated",
                            (),
                        ) {
                            tracing::warn!("history-updated emit failed: {err}");
                        }
                    }
                    Err(err) => {
                        tracing::warn!("history record failed: {err}");
                    }
                }
            }
            match dictionary_manager.lock().await.queue_correction_candidates(
                &correction_candidates_for_dictionary,
                &transcript_for_dictionary_preview,
            ) {
                Ok(added) if added > 0 => {
                    if let Err(err) = app_for_history.emit_to(
                        PILL_WINDOW_LABEL,
                        "voicewave://dictionary-queue-changed",
                        added,
                    ) {
                        tracing::warn!("dictionary-queue-changed emit failed: {err}");
                    }
                }
                Ok(_) => {}
                Err(err) => tracing::warn!("dictionary ingest failed: {err}"),
            }
        });
    }
}
