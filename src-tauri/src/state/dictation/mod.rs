//! The dictation pipeline: one run from the accepted press to inserted text.

mod capture;
mod post_process;
mod preflight;
mod transcribe;

use capture::CapturedAudio;
use post_process::{FormattedText, SpokenText};
use transcribe::{DecodeModeChoice, Decoded, TranscribingStart, Transcription};
use super::{
    clamp_max_utterance_ms, classify_insertion_target, derive_correction_candidates,
    emit_pill_action_notice, emit_pill_notice, emit_pill_rescue, emit_submit_notice,
    insertion_method_key, now_utc_ms, restore_snippet_candidate, return_focus_for_retry,
    sample_count_to_ms, ControllerError, CorrectionSession, DictationControlMode,
    DictationLifecycleState, DictationMode, LatencyBreakdownEvent, PillAction, RetainedAudio,
    TranscriptEvent, VoiceWaveController, VoiceWaveHudState, CORRECTION_SESSION_WINDOW_MS,
    MAIN_WINDOW_LABEL, PILL_WINDOW_LABEL, POLISH_OUTCOME_ACCEPTED, POLISH_OUTCOME_DISABLED,
    POLISH_OUTCOME_FALLBACK_REJECTED, POLISH_OUTCOME_FALLBACK_TIMEOUT, POLISH_OUTCOME_LITERAL,
    POLISH_OUTCOME_SNIPPET_EXACT, POLISH_PILL_ANNOUNCE_EVERY, SUBMIT_DELAY_AFTER_PASTE_MS,
    SUBMIT_DELAY_AFTER_TYPING_MS, WAIT_VALIDATED_RELEASE_BUDGET_MS,
};
use crate::{
    diagnostics::LatencyMetricRecord,
    history::PolishHistoryMeta,
    inference::{polish_gate::gate_polish_candidate, uses_whisper_pipeline},
    insertion::InsertTextRequest,
    settings::{InsertPath, PolishProfile, VoiceWaveSettings},
};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter};
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};
use tokio_util::sync::CancellationToken;
/// One dictation's fixed inputs, shared by every phase.
struct DictationRun {
    app: AppHandle,
    mode: DictationMode,
    session_id: u64,
    control_mode: DictationControlMode,
    cancel_token: CancellationToken,
    /// Set when the user ends the capture (release, second press, Stop).
    stop_flag: Arc<AtomicBool>,
    /// Flips the capture to hands-free limits (at start or on a double-tap lock).
    hands_free: Arc<AtomicBool>,
    /// Kept audio being transcribed again (Try again, plan 020).
    retry_audio: Option<RetainedAudio>,
    flow_started: Instant,
    /// Taken once at flow start, so a mid-flight settings change cannot
    /// produce a hybrid dictation.
    settings: VoiceWaveSettings,
    max_capture_ms: u64,
    /// faster-whisper and the whisper.cpp speech worker share capture,
    /// conditioning, quality gates and finalization (plan 019).
    whisper_pipeline: bool,
}

impl VoiceWaveController {
    pub(super) async fn run_dictation_flow(
        &self,
        app: AppHandle,
        mode: DictationMode,
        session_id: u64,
        control_mode: DictationControlMode,
        cancel_token: CancellationToken,
        stop_flag: Arc<AtomicBool>,
        hands_free: Arc<AtomicBool>,
        retry_audio: Option<RetainedAudio>,
    ) -> Result<(), ControllerError> {
        let flow_started = Instant::now();
        if mode == DictationMode::Fixture {
            // Sound cue is now played from the pill renderer on state commit
            // so the sound lines up with the visible pill onset.
            self.update_state(
                &app,
                VoiceWaveHudState::Listening,
                Some("Listening for speech...".to_string()),
            )
            .await;
        }
        if retry_audio.is_none() {
            self.set_session_state(session_id, DictationLifecycleState::Listening, None, None)
                .await;
        }

        let settings = self.settings.lock().await.clone();
        let run = DictationRun {
            max_capture_ms: clamp_max_utterance_ms(settings.max_utterance_ms),
            whisper_pipeline: uses_whisper_pipeline(&settings.active_model),
            app,
            mode,
            session_id,
            control_mode,
            cancel_token,
            stop_flag,
            hands_free,
            retry_audio,
            flow_started,
            settings,
        };
        let DictationRun {
            ref app,
            mode,
            session_id,
            ref retry_audio,
            flow_started,
            ref settings,
            ..
        } = run;
        let fw_ready_task = self.preflight(&run).await?;
        let Some(capture) = self.capture_audio(&run).await? else {
            return Ok(());
        };
        let CapturedAudio {
            ref segments,
            ref source_samples,
            capture_ms,
            ref mic_input_volume_slot,
            ..
        } = capture;
        let Some(transcription) = self.transcribe(&run, &capture, fw_ready_task).await? else {
            return Ok(());
        };
        let Some(spoken) = self
            .apply_submit_command(&run, &capture, &transcription)
            .await
        else {
            return Ok(());
        };
        let Some(text) = self
            .format_transcript(&run, &capture, &transcription, &spoken)
            .await?
        else {
            return Ok(());
        };
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
                    transcribing_started,
                    release_to_transcribing_ms,
                    effective_release_watchdog_ms,
                    watchdog_recovered,
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
        let SpokenText {
            baseline_transcript,
            submit_requested,
            ..
        } = spoken;
        let FormattedText {
            polish_profile,
            profile_policy,
            custom_terms,
            exact_snippet_expansion,
            inline_snippet_plan,
            had_snippet,
            final_transcript,
            asr_integrity_percent,
            asr_raw_word_count,
            asr_final_word_count,
        } = text;

        // ---- Plan 010 delivery policy -----------------------------------
        // Standard keeps today's contract byte-for-byte (deterministic text
        // inserts immediately; polish runs async as a Copy-offer after
        // insertion). Coding/Writing/Casual opted in to "the profile IS the
        // text": ONE blocking LLM attempt through the polish gate, bounded by
        // a hard release-to-insert budget; on accept the polished text
        // inserts, on reject/timeout/worker-error the deterministic floor
        // inserts. Literal never calls the model.
        let mut inserted_text = if let Some(expansion) = &exact_snippet_expansion {
            expansion.clone()
        } else {
            restore_snippet_candidate(inline_snippet_plan.as_deref(), &final_transcript)
                .unwrap_or_else(|error| {
                    tracing::warn!("snippet restoration failed before insertion: {error}");
                    baseline_transcript.clone()
                })
        };
        let mut polish_meta = PolishHistoryMeta {
            selected_profile: Some(polish_profile.as_str().to_string()),
            ..PolishHistoryMeta::default()
        };
        let mut polish_wait_ms: u64 = 0;
        // A wait-path attempt that misses the budget keeps running in the
        // background; its (gated) result may still enrich History later —
        // never the target app.
        let mut late_polish_handle: Option<JoinHandle<Option<String>>> = None;
        if exact_snippet_expansion.is_some() {
            polish_meta.polish_outcome = Some(POLISH_OUTCOME_SNIPPET_EXACT.to_string());
        } else {
            match profile_policy.insert_path {
                InsertPath::LiteralImmediate => {
                    polish_meta.polish_outcome = Some(POLISH_OUTCOME_LITERAL.to_string());
                }
                InsertPath::Immediate => {
                    // Standard: outcome intentionally stays None — the async
                    // polish pass is an offer, not an insertion outcome.
                }
                InsertPath::WaitValidated => {
                    // No retry pass exists yet (worker support pending); record
                    // that explicitly so History never has to guess.
                    polish_meta.polish_retried = Some(false);
                    if !settings.llm_polish_enabled {
                        polish_meta.polish_outcome = Some(POLISH_OUTCOME_DISABLED.to_string());
                    } else {
                        // Budget zero point == key release, the same point the
                        // existing release_to_inserted_ms metric measures from.
                        let elapsed_since_release_ms = release_to_transcribing_ms
                            .saturating_add(transcribing_started.elapsed().as_millis() as u64);
                        let remaining_budget_ms = WAIT_VALIDATED_RELEASE_BUDGET_MS
                            .saturating_sub(elapsed_since_release_ms);
                        if remaining_budget_ms == 0 {
                            // Decode alone consumed the budget (long dictation):
                            // insert the floor immediately, no attempt.
                            polish_meta.polish_outcome =
                                Some(POLISH_OUTCOME_FALLBACK_TIMEOUT.to_string());
                        } else {
                            let raw_for_polish = final_transcript.clone();
                            let prompt_id = profile_policy.prompt_id.to_string();
                            let wait_started = Instant::now();
                            let mut polish_handle = tokio::spawn(async move {
                                crate::inference::llm_polish::polish_text_for_profile(
                                    raw_for_polish,
                                    prompt_id,
                                )
                                .await
                                .ok()
                                .flatten()
                            });
                            match timeout(
                                Duration::from_millis(remaining_budget_ms),
                                &mut polish_handle,
                            )
                            .await
                            {
                                Ok(Ok(Some(candidate))) => {
                                    polish_wait_ms = wait_started.elapsed().as_millis() as u64;
                                    polish_meta.polish_latency_ms = Some(polish_wait_ms);
                                    match gate_polish_candidate(
                                        &final_transcript,
                                        &candidate,
                                        &custom_terms,
                                        polish_profile,
                                    ) {
                                        Ok(stabilized) => match restore_snippet_candidate(
                                            inline_snippet_plan.as_deref(),
                                            &stabilized,
                                        ) {
                                            Ok(restored) => {
                                                polish_meta.polish_outcome =
                                                    Some(POLISH_OUTCOME_ACCEPTED.to_string());
                                                polish_meta.polished_text = Some(restored.clone());
                                                inserted_text = restored;
                                            }
                                            Err(error) => {
                                                tracing::info!(
                                                    "{} polish changed a protected snippet slot ({error}); inserting deterministic floor",
                                                    polish_profile.as_str()
                                                );
                                                polish_meta.polish_outcome = Some(
                                                    POLISH_OUTCOME_FALLBACK_REJECTED.to_string(),
                                                );
                                            }
                                        },
                                        Err(reasons) => {
                                            tracing::info!(
                                                "{} polish rejected ({reasons:?}); inserting deterministic floor",
                                                polish_profile.as_str()
                                            );
                                            polish_meta.polish_outcome =
                                                Some(POLISH_OUTCOME_FALLBACK_REJECTED.to_string());
                                        }
                                    }
                                }
                                Ok(Ok(None)) | Ok(Err(_)) => {
                                    // Worker error / missing model / empty output.
                                    polish_wait_ms = wait_started.elapsed().as_millis() as u64;
                                    polish_meta.polish_latency_ms = Some(polish_wait_ms);
                                    polish_meta.polish_outcome =
                                        Some(POLISH_OUTCOME_FALLBACK_REJECTED.to_string());
                                }
                                Err(_elapsed) => {
                                    polish_wait_ms = wait_started.elapsed().as_millis() as u64;
                                    polish_meta.polish_latency_ms = Some(polish_wait_ms);
                                    polish_meta.polish_outcome =
                                        Some(POLISH_OUTCOME_FALLBACK_TIMEOUT.to_string());
                                    late_polish_handle = Some(polish_handle);
                                }
                            }
                        }
                    }
                }
            }
        }
        polish_meta.inserted_text = Some(inserted_text.clone());

        let post_started = Instant::now();
        let _ = app.emit(
            "voicewave://transcript",
            TranscriptEvent {
                text: inserted_text.clone(),
                is_final: true,
                elapsed_ms: 0,
            },
        );

        {
            let mut snapshot = self.snapshot.lock().await;
            snapshot.last_partial = None;
            snapshot.last_final = Some(inserted_text.clone());
            snapshot.active_model = settings.active_model.clone();
        }
        let post_before_insert_ms = post_started.elapsed().as_millis() as u64;

        return_focus_for_retry(retry_audio.as_ref()).await;

        let insert_payload = InsertTextRequest {
            text: inserted_text.clone(),
            target_app: None,
            prefer_clipboard: settings.prefer_clipboard_fallback
                || (had_snippet && inserted_text.contains('\n')),
            force_clipboard_only: false,
        };
        let insert_started = Instant::now();
        let (
            insertion_success,
            insertion_method,
            insertion_target_class,
            insertion_history_recorded,
            insertion_record_id,
            insertion_blocked_by_elevation,
        ) = match self
            .insert_text_with_polish_meta(app.clone(), insert_payload, Some(&polish_meta))
            .await
        {
            Ok((result, record_id)) => (
                result.success,
                insertion_method_key(&result.method).to_string(),
                classify_insertion_target(result.target_app.as_deref()).to_string(),
                // insert_text already wrote the history record (with method
                // and success), or logged why it could not; the record is in
                // memory either way, so the transcript-side write below must
                // not duplicate it.
                true,
                record_id,
                result.blocked_by_elevation,
            ),
            Err(err) => {
                self.update_state(
                    &app,
                    VoiceWaveHudState::Error,
                    Some(format!("Insertion failed: {err}")),
                )
                .await;
                self.set_session_state(session_id, DictationLifecycleState::Error, None, None)
                    .await;
                (
                    false,
                    "error".to_string(),
                    "unknown".to_string(),
                    false,
                    None,
                    false,
                )
            }
        };
        let insert_ms = insert_started.elapsed().as_millis() as u64;
        // Spoken submit: Enter goes only into the window that received the
        // text, after the paste has had time to land (see the delay consts).
        let submit_outcome = if submit_requested {
            let delay_ms = match insertion_method.as_str() {
                "clipboardPaste" => SUBMIT_DELAY_AFTER_PASTE_MS,
                "direct" => SUBMIT_DELAY_AFTER_TYPING_MS,
                _ => 0,
            };
            sleep(Duration::from_millis(delay_ms)).await;
            Some(self.insertion_engine.lock().await.press_enter_after_insert())
        } else {
            None
        };
        let inserted_at_utc_ms = now_utc_ms();
        let previous_correction_session = { self.correction_session.lock().await.clone() };
        let correction_candidates = if insertion_success {
            if let Some(previous) = previous_correction_session {
                if !had_snippet
                    && !previous.had_snippet
                    && inserted_at_utc_ms.saturating_sub(previous.inserted_at_utc_ms)
                        <= CORRECTION_SESSION_WINDOW_MS
                {
                    derive_correction_candidates(&previous.inserted_text, &inserted_text)
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };
        {
            let mut correction_session = self.correction_session.lock().await;
            if insertion_success {
                *correction_session = Some(CorrectionSession {
                    inserted_text: inserted_text.clone(),
                    inserted_at_utc_ms,
                    had_snippet,
                });
            } else {
                *correction_session = None;
            }
        }

        // Off-by-default on-device LLM polish (plan 005, Phase 3) — Standard
        // profile only (wait-validated profiles already made their single
        // attempt inline, and Literal never calls the model). Only when the
        // experimental flag is ON and the deterministic text actually landed.
        // This runs in a background task spawned AFTER insertion, so it adds
        // ZERO release-to-text latency and never touches the ASR worker/gate. It
        // OFFERS the polished text via the pill's Copy action; it never
        // overwrites the inserted text, and a rewrite is surfaced only if the
        // fidelity validator passes. With the flag off (default) nothing spawns
        // and behavior is byte-identical to before.
        if insertion_success
            && settings.llm_polish_enabled
            && profile_policy.insert_path == InsertPath::Immediate
            && exact_snippet_expansion.is_none()
        {
            let app_handle = app.clone();
            let raw = final_transcript.clone();
            let polish_count = Arc::clone(&self.polish_success_count);
            let custom_terms_for_polish = custom_terms.clone();
            let session_counter = Arc::clone(&self.session_counter);
            let history_manager = Arc::clone(&self.history_manager);
            let record_id_for_polish = insertion_record_id.clone();
            let snippet_plan_for_polish = inline_snippet_plan.clone();
            tauri::async_runtime::spawn(async move {
                let polish_started = Instant::now();
                if let Ok(Some(polished)) = crate::inference::llm_polish::polish_text_for_profile(
                    raw.clone(),
                    PolishProfile::Standard.as_str().to_string(),
                )
                .await
                {
                    // Re-apply the dictionary-term stabilizer to the LLM output,
                    // then gate strictly on the fidelity validator.
                    let Ok(stabilized) = gate_polish_candidate(
                        &raw,
                        &polished,
                        &custom_terms_for_polish,
                        PolishProfile::Standard,
                    ) else {
                        return;
                    };
                    if stabilized.trim() == raw.trim() {
                        return;
                    }
                    let Ok(restored) =
                        restore_snippet_candidate(snippet_plan_for_polish.as_deref(), &stabilized)
                    else {
                        return;
                    };
                    let polish_latency_ms = polish_started.elapsed().as_millis() as u64;
                    // Enrich this dictation's History record (keyed by its
                    // stable record id, so this is never stale) with the
                    // validated candidate.
                    if let Some(record_id) = record_id_for_polish {
                        let update = PolishHistoryMeta {
                            polished_text: Some(restored.clone()),
                            polish_latency_ms: Some(polish_latency_ms),
                            ..PolishHistoryMeta::default()
                        };
                        if history_manager
                            .lock()
                            .await
                            .update_polish_meta(&record_id, &update)
                            .is_ok()
                        {
                            let _ = app_handle.emit_to(
                                MAIN_WINDOW_LABEL,
                                "voicewave://history-updated",
                                (),
                            );
                        }
                    }
                    // Session-aware offer: if a newer dictation started while
                    // this polish ran, the offer is obsolete — drop it
                    // silently (no stale pill offers).
                    if session_counter.load(Ordering::Relaxed) != session_id {
                        return;
                    }
                    // Polish succeeds on nearly every dictation, so we do NOT
                    // pop the pill each time — that would bury the pill's
                    // important fallback/error messages in noise. Announce
                    // only once every POLISH_PILL_ANNOUNCE_EVERY validated
                    // polishes as a quiet "on-device polishing is working"
                    // signal. `fetch_add` returns the pre-increment value, so
                    // the first success (0) announces immediately, then every
                    // 50th thereafter.
                    let nth = polish_count.fetch_add(1, Ordering::Relaxed);
                    if nth % POLISH_PILL_ANNOUNCE_EVERY == 0 {
                        emit_pill_rescue(
                            &app_handle,
                            "info",
                            "Polished version ready",
                            None,
                            restored,
                            Some("copyTranscript"),
                        );
                    }
                }
            });
        }

        // Wait-path attempt that missed the release budget: let it finish in
        // the background and, if the gate accepts it, record the candidate on
        // this dictation's History record (`polishedText` only — the outcome
        // stays `fallbackTimeout` and the target app is never touched).
        if let (Some(handle), Some(record_id)) = (late_polish_handle, insertion_record_id.clone()) {
            let history_manager = Arc::clone(&self.history_manager);
            let app_for_late = app.clone();
            let raw = final_transcript.clone();
            let custom_terms_late = custom_terms.clone();
            let snippet_plan_late = inline_snippet_plan.clone();
            tauri::async_runtime::spawn(async move {
                if let Ok(Some(candidate)) = handle.await {
                    if let Ok(stabilized) =
                        gate_polish_candidate(&raw, &candidate, &custom_terms_late, polish_profile)
                    {
                        let Ok(restored) =
                            restore_snippet_candidate(snippet_plan_late.as_deref(), &stabilized)
                        else {
                            return;
                        };
                        let update = PolishHistoryMeta {
                            polished_text: Some(restored),
                            ..PolishHistoryMeta::default()
                        };
                        if history_manager
                            .lock()
                            .await
                            .update_polish_meta(&record_id, &update)
                            .is_ok()
                        {
                            let _ = app_for_late.emit_to(
                                MAIN_WINDOW_LABEL,
                                "voicewave://history-updated",
                                (),
                            );
                        }
                    }
                }
            });
        }

        // Rescue pill (Wispr Flow fallback pattern): when the text did not
        // land in the target app, the pill expands with the transcript so the
        // words are visibly safe and one keystroke/click away.
        if mode == DictationMode::Microphone {
            if !insertion_success {
                emit_pill_rescue(
                    &app,
                    "warning",
                    "Couldn't insert — your text is safe",
                    Some("Click Copy, then paste it where you need it.".to_string()),
                    inserted_text.clone(),
                    Some("copyTranscript"),
                );
            } else if insertion_method == "clipboardOnly" {
                let paste_hint = if submit_requested {
                    "Press Ctrl+V to paste it, then Enter."
                } else {
                    "Press Ctrl+V to paste it here."
                };
                if insertion_blocked_by_elevation {
                    emit_pill_rescue(
                        &app,
                        "info",
                        "This app runs as administrator",
                        Some(format!(
                            "VoiceWave can't type into it, so your text is on the clipboard. {paste_hint}"
                        )),
                        inserted_text.clone(),
                        None,
                    );
                } else {
                    emit_pill_rescue(
                        &app,
                        "info",
                        "Copied to clipboard",
                        Some(paste_hint.to_string()),
                        inserted_text.clone(),
                        None,
                    );
                }
            } else if let Some(Err(err)) = &submit_outcome {
                emit_submit_notice(&app, err);
            } else if asr_final_word_count >= 30 {
                // Success stays quiet for everyday dictations (Wispr-style
                // restraint); only big ones earn a word-count confirmation.
                emit_pill_notice(
                    &app,
                    "info",
                    format!("Inserted · {asr_final_word_count} words"),
                    None,
                    2_600,
                );
            }

            // Opt-in one-tap "Add to dictionary?" suggestion. Restraint gate:
            // off by default (setting), and only when there is exactly one
            // high-confidence correction candidate — the dictionary review
            // popup already ingests these independently, so this never
            // double-surfaces on the default calm UX.
            if settings.pill_action_suggestions
                && insertion_success
                && correction_candidates.len() == 1
            {
                let term = correction_candidates[0].clone();
                emit_pill_action_notice(
                    &app,
                    "info",
                    "Add to dictionary?",
                    None,
                    PillAction {
                        kind: "addDictionaryTerm".to_string(),
                        label: format!("Add \"{term}\""),
                        value: Some(term),
                    },
                    6_000,
                );
            }
        }

        self.set_session_state(session_id, DictationLifecycleState::Inserted, None, None)
            .await;

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

        Ok(())
    }
}
