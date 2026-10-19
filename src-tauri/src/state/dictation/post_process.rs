//! Post-processing: the spoken "press enter" command, snippets, and the
//! deterministic formatting stack for the active polish profile, then the
//! empty-result and no-speech gates.

use super::capture::CapturedAudio;
use super::transcribe::{Decoded, Transcription};
use super::DictationRun;
use crate::inference::note_audio_pipeline_decode_hard_failure;
use crate::settings::{resolve_profile, EffectivePolicy, InsertPath, PolishProfile};
use crate::snippet::{ProtectionError, ProtectionOutcome, SnippetExpansionPlan};
use crate::state::{
    asr_integrity_metrics, dictation_retry_instruction, emit_pill_notice, emit_submit_notice,
    return_focus_for_retry, should_reject_low_confidence_transcript_as_no_speech, ControllerError,
    DictationLifecycleState, DictationMode, VoiceWaveController, VoiceWaveHudState,
};
use crate::transcript::{
    finalize_literal_transcript, finalize_pro_transcript, sanitize_user_transcript,
    split_trailing_submit_command, ProTranscriptOptions,
};
use std::sync::Arc;

pub(super) struct SpokenText {
    /// The ASR text minus a trailing "press enter".
    pub(super) merged_transcript: String,
    /// `merged_transcript` sanitized again; the literal fallback text.
    pub(super) baseline_transcript: String,
    pub(super) submit_requested: bool,
}

pub(super) struct FormattedText {
    pub(super) polish_profile: PolishProfile,
    pub(super) profile_policy: EffectivePolicy,
    pub(super) custom_terms: Vec<String>,
    /// The whole dictation was one snippet trigger: insert its expansion.
    pub(super) exact_snippet_expansion: Option<String>,
    /// Inline snippet slots protected through formatting and polish.
    pub(super) inline_snippet_plan: Option<Arc<SnippetExpansionPlan>>,
    pub(super) had_snippet: bool,
    /// Deterministic text, snippet slots still protected.
    pub(super) final_transcript: String,
    pub(super) asr_integrity_percent: f32,
    pub(super) asr_raw_word_count: u32,
    pub(super) asr_final_word_count: u32,
}

impl VoiceWaveController {
    /// Splits off a trailing "press enter" (plan 020). On its own it presses
    /// Enter and returns `None`, ending the flow.
    pub(super) async fn apply_submit_command(
        &self,
        run: &DictationRun,
        capture: &CapturedAudio,
        transcription: &Transcription,
    ) -> Option<SpokenText> {
        let DictationRun {
            ref app,
            mode,
            session_id,
            control_mode,
            ref retry_audio,
            ref settings,
            whisper_pipeline,
            ..
        } = *run;
        let captured_audio_ms = capture.captured_audio_ms;
        let Decoded {
            telemetry: ref decode_telemetry,
            ref finalize_text,
            ..
        } = transcription.decoded;
        let merged_transcript = finalize_text.clone();
        // Spoken submit (plan 020): a trailing "press enter" is a command, not
        // text. It rides the spoken edit commands toggle like "new line".
        let (merged_transcript, submit_requested) = if settings.spoken_edit_commands {
            split_trailing_submit_command(&merged_transcript)
        } else {
            (merged_transcript, false)
        };
        let baseline_transcript = sanitize_user_transcript(&merged_transcript);
        if submit_requested && baseline_transcript.is_empty() {
            // "press enter" on its own: no text, just the key. The no-speech
            // gate still runs on what was heard, so noise can't press Enter.
            let unheard = mode == DictationMode::Microphone
                && should_reject_low_confidence_transcript_as_no_speech(
                    whisper_pipeline,
                    captured_audio_ms,
                    &finalize_text,
                    decode_telemetry.fw_no_speech_prob,
                );
            let outcome = if unheard {
                None
            } else {
                return_focus_for_retry(retry_audio.as_ref()).await;
                Some(self.insertion_engine.lock().await.press_enter_now())
            };
            self.set_session_state(session_id, DictationLifecycleState::Inserted, None, None)
                .await;
            let message = match &outcome {
                Some(Ok(())) => "Pressed Enter.".to_string(),
                Some(Err(_)) => "Didn't press Enter.".to_string(),
                None => format!(
                    "No speech detected. {}",
                    dictation_retry_instruction(control_mode)
                ),
            };
            let hud_state = if matches!(outcome, Some(Ok(()))) {
                VoiceWaveHudState::Inserted
            } else {
                VoiceWaveHudState::Idle
            };
            self.update_state(&app, hud_state, Some(message)).await;
            if let Some(Err(err)) = &outcome {
                emit_submit_notice(&app, err);
            }
            return None;
        }
        Some(SpokenText {
            merged_transcript,
            baseline_transcript,
            submit_requested,
        })
    }

    /// Applies snippets and the profile's deterministic formatting. `None`
    /// means the session already ended (cancelled with nothing left, or no
    /// speech); an empty result that was not cancelled is an error.
    pub(super) async fn format_transcript(
        &self,
        run: &DictationRun,
        capture: &CapturedAudio,
        transcription: &Transcription,
        spoken: &SpokenText,
    ) -> Result<Option<FormattedText>, ControllerError> {
        let DictationRun {
            ref app,
            mode,
            session_id,
            control_mode,
            ref cancel_token,
            ref settings,
            whisper_pipeline,
            ..
        } = *run;
        let captured_audio_ms = capture.captured_audio_ms;
        let Decoded {
            telemetry: ref decode_telemetry,
            ref finalize_text,
            decode_ms,
            ..
        } = transcription.decoded;
        let SpokenText {
            ref merged_transcript,
            ref baseline_transcript,
            ..
        } = *spoken;
        // Plan 010: the persisted polish profile is the single delivery
        // authority for this dictation. Resolve it once against the settings
        // snapshot taken at flow start so a mid-flight profile switch cannot
        // produce a hybrid.
        let polish_profile = settings.effective_polish_profile();
        let profile_policy = resolve_profile(polish_profile, &settings);
        // Dictionary terms feed every branch below (Literal stabilization,
        // the pro deterministic stack, and the polish gate) — fetch once.
        let custom_terms = self
            .dictionary_manager
            .lock()
            .await
            .get_terms(None)
            .into_iter()
            .map(|row| row.term)
            .collect::<Vec<_>>();
        let active_snippets = self.snippet_manager.lock().await.list_snippets(None);
        let protection = match SnippetExpansionPlan::protect(&baseline_transcript, &active_snippets)
        {
            Ok(outcome) => outcome,
            Err(ProtectionError::TooManyMatches) => {
                emit_pill_notice(
                    &app,
                    "warning",
                    "Too many snippet triggers",
                    Some("No snippets were expanded in this dictation.".to_string()),
                    3_500,
                );
                ProtectionOutcome::NoMatch
            }
            Err(error) => {
                tracing::warn!("snippet protection skipped: {error}");
                ProtectionOutcome::NoMatch
            }
        };
        let (exact_snippet_expansion, inline_snippet_plan) = match protection {
            ProtectionOutcome::NoMatch => (None, None),
            ProtectionOutcome::ExactOnly { expansion } => (Some(expansion), None),
            ProtectionOutcome::Inline(plan) => (None, Some(Arc::new(plan))),
        };
        let had_snippet = exact_snippet_expansion.is_some() || inline_snippet_plan.is_some();
        let deterministic_input = inline_snippet_plan
            .as_ref()
            .map(|plan| plan.protected_text())
            .unwrap_or(&merged_transcript);
        let literal_input = inline_snippet_plan
            .as_ref()
            .map(|plan| plan.protected_text())
            .unwrap_or(&baseline_transcript);
        let entitlement = self.billing_manager.lock().await.snapshot();
        let deterministic_transcript = if let Some(expansion) = &exact_snippet_expansion {
            expansion.clone()
        } else if profile_policy.insert_path == InsertPath::LiteralImmediate {
            // Literal branches BEFORE finalize_pro_transcript, from the
            // sanitized ASR baseline: by the end of the deterministic stack
            // the original word sequence (fillers, repeats) is already gone.
            // Only spoken structural commands, dictionary stabilization, and
            // punctuation/capitalization apply — never filler removal, domain
            // corrections, format profiles, or code mode.
            finalize_literal_transcript(literal_input, settings.spoken_edit_commands, &custom_terms)
        } else if entitlement.is_pro {
            let app_profile_behavior = Self::active_profile_behavior(&settings);
            finalize_pro_transcript(
                deterministic_input,
                &ProTranscriptOptions {
                    format_profile: settings.format_profile,
                    domain_packs: &settings.active_domain_packs,
                    code_mode: &settings.code_mode,
                    preserve_protected_content: inline_snippet_plan.is_some(),
                    post_processing_enabled: settings.pro_post_processing_enabled,
                    spoken_edit_commands: settings.spoken_edit_commands,
                    app_profile_behavior: &app_profile_behavior,
                    custom_terms: &custom_terms,
                },
            )
        } else {
            literal_input.to_string()
        };
        let final_transcript = if let Some(plan) = inline_snippet_plan.as_ref() {
            if plan.validate_candidate(&deterministic_transcript).is_ok() {
                deterministic_transcript
            } else {
                tracing::warn!(
                    "deterministic formatting changed a snippet slot; using protected baseline"
                );
                plan.protected_text().to_string()
            }
        } else {
            deterministic_transcript
        };
        let (asr_integrity_percent, asr_raw_word_count, asr_final_word_count) =
            asr_integrity_metrics(&finalize_text, &baseline_transcript);

        if final_transcript.trim().is_empty() {
            if cancel_token.is_cancelled() {
                self.set_session_state(session_id, DictationLifecycleState::Idle, None, None)
                    .await;
                self.update_state(
                    &app,
                    VoiceWaveHudState::Idle,
                    Some("Dictation cancelled.".to_string()),
                )
                .await;
                return Ok(None);
            }
            if mode == DictationMode::Microphone {
                self.decode_policy.lock().await.record_failure(decode_ms);
                if whisper_pipeline {
                    note_audio_pipeline_decode_hard_failure();
                }
            }
            return Err(ControllerError::Runtime(
                "Inference finished without final transcript.".to_string(),
            ));
        }
        if mode == DictationMode::Microphone
            && should_reject_low_confidence_transcript_as_no_speech(
                whisper_pipeline,
                captured_audio_ms,
                &baseline_transcript,
                decode_telemetry.fw_no_speech_prob,
            )
        {
            self.set_session_state(session_id, DictationLifecycleState::Idle, None, None)
                .await;
            self.update_state(
                &app,
                VoiceWaveHudState::Idle,
                Some(format!(
                    "No speech detected. {}",
                    dictation_retry_instruction(control_mode)
                )),
            )
            .await;
            return Ok(None);
        }
        Ok(Some(FormattedText {
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
        }))
    }
}
