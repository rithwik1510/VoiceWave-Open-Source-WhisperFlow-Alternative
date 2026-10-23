//! Insertion: put the text into the target app (paste or typing; elevated
//! targets fall back to the clipboard inside `insert_text`), press Enter
//! for a spoken submit, track quick re-dictations as dictionary correction
//! candidates, and tell the user how it went.

use super::polish::Delivery;
use super::post_process::{FormattedText, SpokenText};
use super::DictationRun;
use crate::insertion::{InsertTextRequest, SubmitError};
use crate::state::{
    classify_insertion_target, derive_correction_candidates, emit_pill_action_notice,
    emit_pill_notice, emit_pill_rescue, emit_submit_notice, insertion_method_key, now_utc_ms,
    return_focus_for_retry, CorrectionSession, DictationLifecycleState, DictationMode, PillAction,
    VoiceWaveController, VoiceWaveHudState, CORRECTION_SESSION_WINDOW_MS,
    HANDS_FREE_MAX_CAPTURE_MS, SUBMIT_DELAY_AFTER_PASTE_MS, SUBMIT_DELAY_AFTER_TYPING_MS,
};
use crate::telemetry::{self, DictationFailureKind, TelemetryEvent};
use serde::{Deserialize, Serialize};
use std::{
    sync::atomic::Ordering,
    time::{Duration, Instant},
};
use tauri::Emitter;
use tokio::time::sleep;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TranscriptEvent {
    text: String,
    is_final: bool,
    elapsed_ms: u64,
}

pub(super) struct Insertion {
    pub(super) success: bool,
    /// `insertion_method_key` of the method used, or "error".
    pub(super) method: String,
    pub(super) target_class: String,
    /// insert_text wrote the History record (or logged why it could not).
    pub(super) history_recorded: bool,
    pub(super) record_id: Option<String>,
    pub(super) blocked_by_elevation: bool,
    /// The spoken "press enter" result, when one was requested.
    pub(super) submit_outcome: Option<Result<(), SubmitError>>,
    pub(super) correction_candidates: Vec<String>,
    pub(super) post_started: Instant,
    pub(super) post_before_insert_ms: u64,
    pub(super) insert_ms: u64,
}

impl VoiceWaveController {
    /// Never fails: an insertion error moves the HUD and session to Error and
    /// comes back as `success: false`, so the transcript is still rescued.
    /// `None` means the user cancelled before the text went in or before
    /// its Enter, and the session has ended.
    pub(super) async fn insert_transcript(
        &self,
        run: &DictationRun,
        delivery: &Delivery,
        text: &FormattedText,
        spoken: &SpokenText,
    ) -> Option<Insertion> {
        let DictationRun {
            ref app,
            mode,
            session_id,
            ref retry_audio,
            ref settings,
            ..
        } = *run;
        let Delivery {
            ref inserted_text,
            ref polish_meta,
            ..
        } = *delivery;
        let had_snippet = text.had_snippet;
        let submit_requested = spoken.submit_requested;
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

        let typing_allowed = return_focus_for_retry(retry_audio.as_ref()).await;
        if self.end_if_cancelled(run).await {
            return None;
        }
        // Keeps Esc owned past the Inserted state (and the History write
        // inside insert_text) until the Enter below is pressed or cancelled.
        if submit_requested {
            self.submit_pending_session
                .store(session_id, Ordering::SeqCst);
        }

        let insert_payload = InsertTextRequest {
            text: inserted_text.clone(),
            target_app: None,
            prefer_clipboard: settings.prefer_clipboard_fallback
                || (had_snippet && inserted_text.contains('\n')),
            // A retry that could not reach its window goes to the clipboard
            // (the rescue pill shows it); nothing lands, so no Enter follows.
            force_clipboard_only: !typing_allowed,
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
            .insert_text_with_polish_meta(app.clone(), insert_payload, Some(polish_meta))
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
                    app,
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
        // An elevated target's clipboard-only fallback counts as a success.
        // Fixture runs are tests, not user dictations.
        if !insertion_success && mode == DictationMode::Microphone {
            telemetry::record(TelemetryEvent::DictationFailed {
                kind: DictationFailureKind::Insert,
            });
        }
        // Spoken submit: Enter goes only into the window that received the
        // text, after the paste has had time to land (see the delay consts).
        let submit_outcome = if submit_requested {
            let delay_ms = match insertion_method.as_str() {
                "clipboardPaste" => SUBMIT_DELAY_AFTER_PASTE_MS,
                "direct" => SUBMIT_DELAY_AFTER_TYPING_MS,
                _ => 0,
            };
            sleep(Duration::from_millis(delay_ms)).await;
            // The text already landed; a cancel still stops the Enter.
            // cancel_dictation cleared the marker and released Esc; a
            // replaced run only clears its own marker.
            if self.end_if_cancelled(run).await {
                // Only this run's own marker: a stale run never clears a newer one.
                let _ = self.submit_pending_session.compare_exchange(
                    session_id,
                    0,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                );
                return None;
            }
            let outcome = self
                .insertion_engine
                .lock()
                .await
                .press_enter_after_insert();
            let _ = self.submit_pending_session.compare_exchange(
                session_id,
                0,
                Ordering::SeqCst,
                Ordering::SeqCst,
            );
            // Esc is ours to give back only while the run still owns the
            // session (a cancel during the Enter already released it).
            if !self.end_if_cancelled(run).await {
                self.set_hotkey_dictation_active(false).await;
            }
            Some(outcome)
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
                    derive_correction_candidates(&previous.inserted_text, inserted_text)
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
        Some(Insertion {
            success: insertion_success,
            method: insertion_method,
            target_class: insertion_target_class,
            history_recorded: insertion_history_recorded,
            record_id: insertion_record_id,
            blocked_by_elevation: insertion_blocked_by_elevation,
            submit_outcome,
            correction_candidates,
            post_started,
            post_before_insert_ms,
            insert_ms,
        })
    }
}

/// The pill after a microphone insertion: the rescue card with the text
/// when it did not land, clipboard hints (with the admin-app explanation),
/// a failed spoken Enter, a word count for long dictations, and the opt-in
/// "Add to dictionary?" suggestion.
pub(super) fn announce_insertion(
    run: &DictationRun,
    delivery: &Delivery,
    text: &FormattedText,
    spoken: &SpokenText,
    insertion: &Insertion,
    hands_free_cap_hit: bool,
) {
    let DictationRun {
        ref app,
        mode,
        ref settings,
        ..
    } = *run;
    let inserted_text = &delivery.inserted_text;
    let asr_final_word_count = text.asr_final_word_count;
    let submit_requested = spoken.submit_requested;
    let Insertion {
        success: insertion_success,
        method: ref insertion_method,
        blocked_by_elevation: insertion_blocked_by_elevation,
        ref submit_outcome,
        ref correction_candidates,
        ..
    } = *insertion;
    // Rescue pill (Wispr Flow fallback pattern): when the text did not
    // land in the target app, the pill expands with the transcript so the
    // words are visibly safe and one keystroke/click away.
    if mode == DictationMode::Microphone {
        if !insertion_success {
            emit_pill_rescue(
                app,
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
                    app,
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
                    app,
                    "info",
                    "Copied to clipboard",
                    Some(paste_hint.to_string()),
                    inserted_text.clone(),
                    None,
                );
            }
        } else if let Some(Err(err)) = &submit_outcome {
            emit_submit_notice(app, err);
        } else if hands_free_cap_hit {
            // Said here rather than at the cap: the Transcribing state would
            // hide it, and a long decode outlasts any notice timer.
            emit_pill_notice(
                app,
                "info",
                format!(
                    "Reached the {}-minute limit",
                    HANDS_FREE_MAX_CAPTURE_MS / 60_000
                ),
                Some("Dictation stopped there. What you said before it was inserted.".to_string()),
                5_000,
            );
        } else if asr_final_word_count >= 30 {
            // Success stays quiet for everyday dictations (Wispr-style
            // restraint); only big ones earn a word-count confirmation.
            emit_pill_notice(
                app,
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
        if settings.pill_action_suggestions && insertion_success && correction_candidates.len() == 1
        {
            let term = correction_candidates[0].clone();
            emit_pill_action_notice(
                app,
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
}
