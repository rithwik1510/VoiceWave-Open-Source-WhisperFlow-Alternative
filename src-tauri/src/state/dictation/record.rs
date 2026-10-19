//! Recording: latency telemetry (UI event + opt-in diagnostics), the
//! always-on Stats numbers, and the background History rescue write and
//! dictionary-candidate queue.

use super::capture::CapturedAudio;
use super::insert::Insertion;
use super::polish::Delivery;
use super::post_process::FormattedText;
use super::transcribe::{DecodeModeChoice, Decoded, TranscribingStart, Transcription};
use super::DictationRun;
use crate::diagnostics::LatencyMetricRecord;
use crate::history::PolishHistoryMeta;
use crate::state::{
    now_utc_ms, sample_count_to_ms, DictationMode, LatencyBreakdownEvent, VoiceWaveController,
    MAIN_WINDOW_LABEL, PILL_WINDOW_LABEL,
};
use std::sync::Arc;
use tauri::Emitter;

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
            ref segments,
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
        let segments_captured = segments.len() as u32;
        let release_stop_detected_at_utc_ms = self
            .session_release_stop_detected_at_utc_ms(session_id)
            .await
            .unwrap_or(0);
        let _ = app.emit(
            "voicewave://latency",
            LatencyBreakdownEvent {
                session_id,
                capture_ms,
                release_to_transcribing_ms,
                effective_release_watchdog_ms,
                watchdog_recovered,
                segments_captured,
                release_stop_detected_at_utc_ms,
                model_init_ms: decode_telemetry.model_init_ms,
                audio_condition_ms: decode_telemetry.audio_condition_ms,
                decode_compute_ms: decode_telemetry.decode_compute_ms,
                runtime_cache_hit: decode_telemetry.runtime_cache_hit,
                backend_requested: decode_telemetry.backend_requested.clone(),
                backend_used: decode_telemetry.backend_used.clone(),
                backend_fallback: decode_telemetry.backend_fallback,
                asr_engine: decode_telemetry.asr_engine.clone(),
                asr_step_down_reason: decode_telemetry.asr_step_down_reason.clone(),
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
                decode_policy_mode_selected: policy_selected_mode,
                decode_policy_reason: decode_policy_reason.clone(),
                fw_low_coherence: decode_telemetry.fw_low_coherence,
                fw_retry_used: decode_telemetry.fw_retry_used,
                fw_literal_retry_used: decode_telemetry.fw_literal_retry_used,
                audio_pipeline_version: decode_telemetry.audio_pipeline_version.clone(),
                fw_avg_logprob: decode_telemetry.fw_avg_logprob,
                fw_no_speech_prob: decode_telemetry.fw_no_speech_prob,
                fw_compression_ratio: decode_telemetry.fw_compression_ratio,
                fw_shadow_candidate_version: decode_telemetry.fw_shadow_candidate_version.clone(),
                fw_shadow_quality_delta: decode_telemetry.fw_shadow_quality_delta,
                fw_shadow_candidate_avg_logprob: decode_telemetry.fw_shadow_candidate_avg_logprob,
                fw_shadow_candidate_no_speech_prob: decode_telemetry
                    .fw_shadow_candidate_no_speech_prob,
                fw_shadow_candidate_retry_used: decode_telemetry.fw_shadow_candidate_retry_used,
                fw_shadow_candidate_low_coherence: decode_telemetry
                    .fw_shadow_candidate_low_coherence,
                fw_shadow_candidate_decode_compute_ms: decode_telemetry
                    .fw_shadow_candidate_decode_compute_ms,
                fw_shadow_candidate_won: decode_telemetry.fw_shadow_candidate_won,
                audio_pipeline_fallback_engaged: decode_telemetry.audio_pipeline_fallback_engaged,
                audio_pipeline_fallback_remaining: decode_telemetry
                    .audio_pipeline_fallback_remaining,
                warm_start_hit: decode_telemetry.runtime_cache_hit,
                worker_reused: decode_telemetry.runtime_cache_hit,
                correction_candidates_count: correction_candidates.len() as u32,
                insertion_method: insertion_method.clone(),
                insertion_target_class: insertion_target_class.clone(),
                mic_input_volume_percent: mic_input_volume_slot.lock().ok().and_then(|slot| *slot),
            },
        );
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
                Some(insertion_target_class.as_str()),
                decode_telemetry.fw_avg_logprob,
                decode_telemetry.fw_no_speech_prob,
            ) {
                tracing::warn!("stats record failed: {err}");
            }
        }
        if settings.diagnostics_opt_in {
            let _ = self
                .diagnostics_manager
                .lock()
                .await
                .record_latency(LatencyMetricRecord {
                    session_id,
                    timestamp_utc_ms: now_utc_ms(),
                    capture_ms,
                    release_to_transcribing_ms,
                    effective_release_watchdog_ms,
                    watchdog_recovered,
                    segments_captured,
                    release_stop_detected_at_utc_ms,
                    model_init_ms: decode_telemetry.model_init_ms,
                    audio_condition_ms: decode_telemetry.audio_condition_ms,
                    decode_compute_ms: decode_telemetry.decode_compute_ms,
                    runtime_cache_hit: decode_telemetry.runtime_cache_hit,
                    backend_requested: decode_telemetry.backend_requested,
                    backend_used: decode_telemetry.backend_used,
                    backend_fallback: decode_telemetry.backend_fallback,
                    asr_engine: Some(decode_telemetry.asr_engine)
                        .filter(|engine| !engine.is_empty()),
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
                    fw_shadow_candidate_avg_logprob: decode_telemetry
                        .fw_shadow_candidate_avg_logprob,
                    fw_shadow_candidate_no_speech_prob: decode_telemetry
                        .fw_shadow_candidate_no_speech_prob,
                    fw_shadow_candidate_retry_used: decode_telemetry.fw_shadow_candidate_retry_used,
                    fw_shadow_candidate_low_coherence: decode_telemetry
                        .fw_shadow_candidate_low_coherence,
                    fw_shadow_candidate_decode_compute_ms: decode_telemetry
                        .fw_shadow_candidate_decode_compute_ms,
                    fw_shadow_candidate_won: decode_telemetry.fw_shadow_candidate_won,
                    audio_pipeline_fallback_engaged: decode_telemetry
                        .audio_pipeline_fallback_engaged,
                    audio_pipeline_fallback_remaining: decode_telemetry
                        .audio_pipeline_fallback_remaining,
                    warm_start_hit: decode_telemetry.runtime_cache_hit,
                    worker_reused: decode_telemetry.runtime_cache_hit,
                    correction_candidates_count: correction_candidates.len() as u32,
                    insertion_method: Some(insertion_method),
                    insertion_target_class: Some(insertion_target_class),
                    success: insertion_success,
                    mic_input_volume_percent: mic_input_volume_slot
                        .lock()
                        .ok()
                        .and_then(|slot| *slot),
                });
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
