//! Preflight: fail fast on a missing model and start warming the speech
//! engine while the user speaks.

use super::DictationRun;
use crate::inference::{
    ensure_faster_whisper_ready, is_faster_whisper_model, is_whisper_cpp_model,
    warmup_faster_whisper_model, InferenceError,
};
use crate::state::{ControllerError, DictationMode, VoiceWaveController};
use tokio::task::JoinHandle;

/// faster-whisper runtime readiness (plus GPU warmup), joined after capture.
pub(super) type FwReadyTask = JoinHandle<Result<(), InferenceError>>;

impl VoiceWaveController {
    /// Runs before capture starts. Errors only on a missing model.
    pub(super) async fn preflight(
        &self,
        run: &DictationRun,
    ) -> Result<Option<FwReadyTask>, ControllerError> {
        let DictationRun {
            mode,
            ref settings,
            whisper_pipeline,
            ..
        } = *run;
        let use_faster_whisper = is_faster_whisper_model(&settings.active_model);
        let use_asr_worker = is_whisper_cpp_model(&settings.active_model);
        // Fail on the press, not after the user has spoken a whole sentence: a
        // model only counts once its download finished and was registered (a
        // half-downloaded model must never reach a worker).
        if mode == DictationMode::Microphone
            && whisper_pipeline
            && self
                .model_manager
                .lock()
                .await
                .get_installed(&settings.active_model)
                .is_none()
        {
            return Err(ControllerError::MissingModel(settings.active_model.clone()));
        }
        let fw_ready_task = if mode == DictationMode::Microphone && use_faster_whisper {
            let warmup_model_id = settings.active_model.clone();
            Some(tokio::spawn(async move {
                let ready = ensure_faster_whisper_ready().await;
                if ready.is_ok() {
                    // GPU clock ramp + kernel warmup runs while the user is
                    // still speaking, so the real decode at release starts on
                    // a hot GPU instead of paying the idle penalty inline.
                    // Skipped automatically when the GPU was active recently.
                    let _ = warmup_faster_whisper_model(&warmup_model_id).await;
                }
                ready
            }))
        } else {
            None
        };
        if mode == DictationMode::Microphone && use_asr_worker {
            // Spawn/load the speech worker while the user speaks (startup
            // prewarm missed, worker died, or a GPU failure dropped it). A
            // no-op when it is already loaded.
            self.spawn_asr_worker_prewarm(&settings.active_model).await;
        }
        Ok(fw_ready_task)
    }
}
