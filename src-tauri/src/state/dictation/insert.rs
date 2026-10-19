//! Insertion: put the text into the target app (paste or typing; elevated
//! targets fall back to the clipboard inside `insert_text`), press Enter
//! for a spoken submit, and track quick re-dictations as dictionary
//! correction candidates.

use super::polish::Delivery;
use super::post_process::{FormattedText, SpokenText};
use super::DictationRun;
use crate::insertion::{InsertTextRequest, SubmitError};
use crate::state::{
    classify_insertion_target, derive_correction_candidates, insertion_method_key, now_utc_ms,
    return_focus_for_retry, CorrectionSession, DictationLifecycleState, TranscriptEvent,
    VoiceWaveController, VoiceWaveHudState, CORRECTION_SESSION_WINDOW_MS,
    SUBMIT_DELAY_AFTER_PASTE_MS, SUBMIT_DELAY_AFTER_TYPING_MS,
};
use std::time::{Duration, Instant};
use tauri::Emitter;
use tokio::time::sleep;

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
    pub(super) async fn insert_transcript(
        &self,
        run: &DictationRun,
        delivery: &Delivery,
        text: &FormattedText,
        spoken: &SpokenText,
    ) -> Insertion {
        let DictationRun {
            ref app,
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
        Insertion {
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
        }
    }
}
