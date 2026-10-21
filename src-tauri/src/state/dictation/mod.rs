//! The dictation pipeline: one run from the accepted press to inserted text.
//! `run_dictation_flow` orchestrates; each phase lives in its own file.

mod capture;
mod insert;
mod polish;
mod post_process;
mod preflight;
mod record;
mod transcribe;

use super::{
    clamp_max_utterance_ms, ControllerError, DictationControlMode, DictationLifecycleState,
    DictationMode, RetainedAudio, VoiceWaveController, VoiceWaveHudState,
};
use crate::{inference::uses_whisper_pipeline, settings::VoiceWaveSettings};
use insert::announce_insertion;
use polish::resolve_delivery;
use std::{
    sync::{atomic::AtomicBool, Arc},
    time::Instant,
};
use tauri::AppHandle;
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
    /// Runs one dictation, in order:
    /// 1. preflight: missing-model check, speech engine warmup
    /// 2. capture: mic volume guard, mic capture (or the kept audio on Try
    ///    again), audio kept for a retry, audio-quality notices
    /// 3. transcribe: decode mode, engine wait, Transcribing state, timed
    ///    decode (GPU-to-CPU fallback happens inside the worker)
    /// 4. post-process: spoken "press enter", snippets, deterministic
    ///    formatting, empty/no-speech gates
    /// 5. deliver: wait-validated polish, insertion and Enter, background
    ///    polish, outcome pill, Inserted state
    /// 6. record: latency/stats/diagnostics, History rescue, dictionary queue
    ///
    /// A phase that returns `None` has already ended the session (cancelled,
    /// nothing heard, Enter only) and the flow returns `Ok(())`. Errors go to
    /// the caller, which shows them and offers Try again when audio was kept.
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
        let fw_ready_task = self.preflight(&run).await?;
        let Some(capture) = self.capture_audio(&run).await? else {
            return Ok(());
        };
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
        let mut delivery = resolve_delivery(&run, &text, &spoken, &transcription).await;
        let insertion = self
            .insert_transcript(&run, &delivery, &text, &spoken)
            .await;
        self.spawn_background_polish(&run, &text, &insertion, delivery.late_polish_handle.take());
        announce_insertion(
            &run,
            &delivery,
            &text,
            &spoken,
            &insertion,
            transcription.start.hands_free_cap_hit,
        );
        self.set_session_state(
            run.session_id,
            DictationLifecycleState::Inserted,
            None,
            None,
        )
        .await;
        self.record_dictation(&run, &capture, transcription, &text, delivery, insertion)
            .await;
        Ok(())
    }
}
