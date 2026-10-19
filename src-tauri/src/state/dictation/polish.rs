//! AI polish: the wait-validated delivery decision (plan 010) before
//! insertion.

use super::post_process::{FormattedText, SpokenText};
use super::transcribe::{TranscribingStart, Transcription};
use super::DictationRun;
use crate::history::PolishHistoryMeta;
use crate::inference::polish_gate::gate_polish_candidate;
use crate::settings::InsertPath;
use crate::state::{
    restore_snippet_candidate, POLISH_OUTCOME_ACCEPTED, POLISH_OUTCOME_DISABLED,
    POLISH_OUTCOME_FALLBACK_REJECTED, POLISH_OUTCOME_FALLBACK_TIMEOUT, POLISH_OUTCOME_LITERAL,
    POLISH_OUTCOME_SNIPPET_EXACT, WAIT_VALIDATED_RELEASE_BUDGET_MS,
};
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;
use tokio::time::timeout;

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
    Delivery {
        inserted_text,
        polish_meta,
        polish_wait_ms,
        late_polish_handle,
    }
}
