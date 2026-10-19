//! AI polish: the wait-validated delivery decision (plan 010) before
//! insertion, and the background polish work spawned after it.

use super::insert::Insertion;
use super::post_process::{FormattedText, SpokenText};
use super::transcribe::{TranscribingStart, Transcription};
use super::DictationRun;
use crate::history::PolishHistoryMeta;
use crate::inference::polish_gate::gate_polish_candidate;
use crate::settings::{InsertPath, PolishProfile};
use crate::snippet::{ProtectionError, SnippetExpansionPlan};
use crate::state::{emit_pill_rescue, VoiceWaveController, MAIN_WINDOW_LABEL};
use std::sync::{atomic::Ordering, Arc};
use std::time::{Duration, Instant};
use tauri::Emitter;
use tokio::task::JoinHandle;
use tokio::time::timeout;

/// How often the background "Polished version ready" pill is allowed to
/// surface: once per this many validated polishes. LLM polish improves nearly
/// every dictation (a comma, a capital), so announcing each one buries the
/// pill's important messages (fallback/error) in noise. Instead we surface it
/// occasionally as a quiet reminder that on-device polishing is working; the
/// polished text is always available via history regardless.
const POLISH_PILL_ANNOUNCE_EVERY: u64 = 50;

/// Hard release-to-insert budget for the wait-validated profiles
/// (Coding/Writing/Casual, plan 010): measured from key release — the same
/// zero point as the existing `release_to_inserted_ms` metric. If a single
/// polish attempt has not produced an accepted result by this deadline, the
/// deterministic floor text inserts and the outcome is recorded as
/// `fallbackTimeout`. No blocking retry, ever.
const WAIT_VALIDATED_RELEASE_BUDGET_MS: u64 = 3_500;

/// History `polishOutcome` wire values (plan 010 interface contract).
const POLISH_OUTCOME_ACCEPTED: &str = "accepted";
const POLISH_OUTCOME_FALLBACK_TIMEOUT: &str = "fallbackTimeout";
const POLISH_OUTCOME_FALLBACK_REJECTED: &str = "fallbackRejected";
const POLISH_OUTCOME_LITERAL: &str = "literal";
const POLISH_OUTCOME_DISABLED: &str = "disabled";
const POLISH_OUTCOME_SNIPPET_EXACT: &str = "snippetExact";

fn restore_snippet_candidate(
    plan: Option<&SnippetExpansionPlan>,
    candidate: &str,
) -> Result<String, ProtectionError> {
    let restored = match plan {
        Some(plan) => plan.restore(candidate)?,
        None => candidate.to_string(),
    };
    debug_assert!(
        !restored.contains('\u{e000}') && !restored.contains('\u{e001}'),
        "protected snippet marker escaped the delivery boundary"
    );
    Ok(restored)
}

/// What gets inserted, and the polish facts History records about it.
pub(super) struct Delivery {
    pub(super) inserted_text: String,
    pub(super) polish_meta: PolishHistoryMeta,
    /// Time spent waiting on the wait-validated polish attempt.
    pub(super) polish_wait_ms: u64,
    /// A wait-path attempt that missed the budget and is still running.
    pub(super) late_polish_handle: Option<JoinHandle<Option<String>>>,
}

/// Decides the text to insert. Literal and Standard insert the
/// deterministic text; wait-validated profiles make one polish attempt
/// within the release budget and fall back to that text on reject or
/// timeout.
pub(super) async fn resolve_delivery(
    run: &DictationRun,
    text: &FormattedText,
    spoken: &SpokenText,
    transcription: &Transcription,
) -> Delivery {
    let settings = &run.settings;
    let FormattedText {
        polish_profile,
        ref profile_policy,
        ref custom_terms,
        ref exact_snippet_expansion,
        ref inline_snippet_plan,
        ref final_transcript,
        ..
    } = *text;
    let baseline_transcript = &spoken.baseline_transcript;
    let TranscribingStart {
        transcribing_started,
        release_to_transcribing_ms,
        ..
    } = transcription.start;
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
        restore_snippet_candidate(inline_snippet_plan.as_deref(), &final_transcript).unwrap_or_else(
            |error| {
                tracing::warn!("snippet restoration failed before insertion: {error}");
                baseline_transcript.clone()
            },
        )
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
                    let remaining_budget_ms =
                        WAIT_VALIDATED_RELEASE_BUDGET_MS.saturating_sub(elapsed_since_release_ms);
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
                                            polish_meta.polish_outcome =
                                                Some(POLISH_OUTCOME_FALLBACK_REJECTED.to_string());
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
    Delivery {
        inserted_text,
        polish_meta,
        polish_wait_ms,
        late_polish_handle,
    }
}

impl VoiceWaveController {
    /// After insertion: Standard's optional polish offer, and recording a
    /// late wait-path result on History. Both only spawn tasks; neither can
    /// change the inserted text.
    pub(super) fn spawn_background_polish(
        &self,
        run: &DictationRun,
        text: &FormattedText,
        insertion: &Insertion,
        late_polish_handle: Option<JoinHandle<Option<String>>>,
    ) {
        let DictationRun {
            ref app,
            session_id,
            ref settings,
            ..
        } = *run;
        let FormattedText {
            polish_profile,
            ref profile_policy,
            ref custom_terms,
            ref exact_snippet_expansion,
            ref inline_snippet_plan,
            ref final_transcript,
            ..
        } = *text;
        let insertion_success = insertion.success;
        let insertion_record_id = &insertion.record_id;
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
    }
}
