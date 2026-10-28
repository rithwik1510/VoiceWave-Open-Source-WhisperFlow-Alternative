//! Controller surface for plan 019's engine selection: the status the UI
//! shows ("Running on …", "Getting your graphics card ready", the switch to
//! a processor model when the GPU is unusable), finishing a GPU upgrade
//! accepted before 0.7.5 (the Parakeet offer replaced that notice), and
//! removing a model's files.

use super::{ControllerError, VoiceWaveController};
use crate::inference::{
    asr_engine_view,
    gpu_select::{
        clear_pending_upgrade, driver_disabled, hardware_profile_async, model_tier,
        pending_upgrade, CpuSupport, GpuVendor, HardwareProfile, ModelTier, QualOutcome,
        PARAKEET_MODEL_ID,
    },
    is_faster_whisper_model, is_parakeet_model, is_whisper_cpp_model, qualify_gpu, AsrRung,
    RunningOn,
};
use crate::model_manager::InstalledModel;
use crate::settings::GpuAcceleration;
use serde::Serialize;
use tauri::AppHandle;

/// A model the UI offers to download: the processor model for a whisper.cpp
/// model stuck on the CPU.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelOffer {
    pub model_id: String,
    pub tier: ModelTier,
    pub size_bytes: u64,
}

/// What a whisper.cpp user whose GPU is unusable is offered instead.
const CPU_MODEL_ID: &str = "fw-small.en";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechEngineStatus {
    pub cpu_support: CpuSupport,
    /// The GPU dictation uses on GPU-path machines.
    pub gpu_name: Option<String>,
    /// That GPU's maker; `None` without one.
    pub gpu_vendor: Option<GpuVendor>,
    pub recommended_model_id: String,
    /// Where the active model runs, from the worker's own report once it has
    /// decoded. `None` while the one-time GPU check runs or is due.
    pub running_on: Option<RunningOn>,
    /// The one-time GPU check (driver shader compile + test clips) is
    /// running for the active model or the accepted upgrade, or is due for
    /// the active model: never checked on this GPU and driver, so it starts
    /// as soon as the model is activated or the app launches. Counting "due"
    /// keeps the UI from reading the moment before the check starts as ready.
    pub preparing: bool,
    pub qualification: Option<QualOutcome>,
    pub qualification_detail: Option<String>,
    /// GGML model the user accepted the old GPU upgrade to; switched to after
    /// it installs and qualifies.
    pub pending_upgrade: Option<String>,
    /// Set when the active whisper.cpp model is stuck on the processor (GPU
    /// check failed or retrying next session, GPU cooldown, driver disabled,
    /// GPU gone or turned off) and no faster-whisper model is installed to
    /// stand in as its CPU rung, so every dictation runs it on the CPU
    /// worker (Large Turbo: 14-23 s each). Names the model to switch to.
    pub cpu_model_offer: Option<ModelOffer>,
}

impl VoiceWaveController {
    pub async fn speech_engine_status(&self) -> SpeechEngineStatus {
        let hardware = hardware_profile_async().await;
        let settings = self.settings.lock().await.clone();
        let pending = pending_upgrade();
        let active_model = self.installed_asr_model(&settings.active_model).await;
        let pending_model = match &pending {
            Some(model_id) => self.installed_asr_model(model_id).await,
            None => None,
        };
        let gpu = hardware.gpu.clone();
        let (active_view, pending_view, gpu_driver_disabled) = tokio::task::spawn_blocking(move || {
            (
                active_model.as_ref().map(asr_engine_view),
                pending_model.as_ref().map(asr_engine_view),
                gpu.as_ref().is_some_and(driver_disabled),
            )
        })
        .await
        .unwrap_or((None, None, false));

        let active_is_ggml = is_whisper_cpp_model(&settings.active_model);
        let gpu_enabled = settings.gpu_acceleration == GpuAcceleration::Auto;
        // Mirrors when the worker runs the check (`primary_rung_blocking`):
        // GPU on and present, driver not disabled, no result yet for this
        // model on this driver.
        let check_due = active_is_ggml
            && gpu_enabled
            && hardware.gpu.is_some()
            && !gpu_driver_disabled
            && active_view
                .as_ref()
                .is_some_and(|view| !view.preparing && view.qualification.is_none());
        let preparing = check_due
            || active_view.as_ref().is_some_and(|view| view.preparing)
            || pending_view.as_ref().is_some_and(|view| view.preparing);

        let running_on = if !active_is_ggml {
            Some(RunningOn {
                device: AsrRung::Cpu,
                name: None,
            })
        } else if check_due {
            None
        } else {
            active_view.as_ref().and_then(|view| view.running_on.clone())
        };
        // While an accepted upgrade is checked, its result is what the user
        // is waiting for; otherwise the active model's.
        let qualification_view = pending_view.as_ref().or(active_view.as_ref());

        let fw_installed = self
            .model_manager
            .lock()
            .await
            .list_installed()
            .iter()
            .any(|row| is_faster_whisper_model(&row.model_id));

        // Parakeet is meant to run on the processor too, so it gets no offer
        // to switch to faster-whisper Small.
        let stuck_on_cpu = active_is_ggml
            && !is_parakeet_model(&settings.active_model)
            && !preparing
            && hardware.cpu != CpuSupport::Unsupported
            && running_on
                .as_ref()
                .is_some_and(|running| running.device == AsrRung::Cpu);
        let cpu_model_offer = if stuck_on_cpu && !fw_installed {
            self.model_offer(CPU_MODEL_ID).await
        } else {
            None
        };

        let gpu_vendor = hardware.gpu.as_ref().map(|device| device.vendor());
        crate::analytics::note_speech_engine(
            gpu_vendor,
            running_on.as_ref().map(|running| running.device),
            preparing,
        );

        SpeechEngineStatus {
            cpu_support: hardware.cpu,
            gpu_name: hardware.gpu.as_ref().map(|device| device.name.clone()),
            gpu_vendor,
            recommended_model_id: hardware.recommended_model_id().to_string(),
            running_on,
            preparing,
            qualification: qualification_view.and_then(|view| view.qualification),
            qualification_detail: qualification_view.and_then(|view| view.qualification_detail.clone()),
            pending_upgrade: pending,
            cpu_model_offer,
        }
    }

    /// Where Parakeet runs, for its Models note: the processor without a GPU
    /// or with GPU acceleration off, otherwise what its own GPU check
    /// decided. `None` while that is unknown (not installed, check not run
    /// yet or running).
    pub(super) async fn parakeet_running_on(
        &self,
        hardware: &HardwareProfile,
        gpu_acceleration: GpuAcceleration,
    ) -> Option<AsrRung> {
        if hardware.gpu.is_none() || gpu_acceleration == GpuAcceleration::Off {
            return Some(AsrRung::Cpu);
        }
        let model = self.installed_asr_model(PARAKEET_MODEL_ID).await?;
        let view = tokio::task::spawn_blocking(move || asr_engine_view(&model))
            .await
            .ok()?;
        if view.preparing || view.qualification.is_none() {
            return None;
        }
        view.running_on.map(|running| running.device)
    }

    async fn model_offer(&self, model_id: &str) -> Option<ModelOffer> {
        let size_bytes = self
            .model_manager
            .lock()
            .await
            .get_catalog_item(model_id)
            .map(|item| item.size_bytes)?;
        model_tier(model_id).map(|tier| ModelOffer {
            model_id: model_id.to_string(),
            tier,
            size_bytes,
        })
    }

    /// Finish an accepted upgrade once its model is installed: qualify it,
    /// then switch the active model in one settings write. Until then (and
    /// if it fails) faster-whisper keeps dictating. Called after the download
    /// and at startup, so a restart between download and switch resumes it.
    /// A slow or transient result keeps the upgrade pending for next launch;
    /// the third in a row comes back as a fail and ends it.
    pub async fn finish_gpu_upgrade(&self, app: AppHandle) -> Result<(), ControllerError> {
        let Some(model_id) = pending_upgrade() else {
            return Ok(());
        };
        if self.settings.lock().await.active_model == model_id {
            clear_pending_upgrade();
            return Ok(());
        }
        let Some(model) = self.installed_asr_model(&model_id).await else {
            return Ok(());
        };
        let baseline = self.fw_baseline(&model_id).await;
        match qualify_gpu(model, baseline).await {
            Some(QualOutcome::Passed) => {
                // Switch first: if saving settings fails, the upgrade stays
                // pending and is retried at the next launch.
                self.set_active_model(app, model_id.clone()).await?;
                clear_pending_upgrade();
                tracing::info!("GPU upgrade complete, now dictating with {model_id}");
            }
            Some(QualOutcome::Failed) => {
                clear_pending_upgrade();
                tracing::warn!("GPU upgrade to {model_id} failed qualification; staying on faster-whisper");
            }
            _ => {}
        }
        Ok(())
    }

    /// Delete a model that is not active (e.g. an upgrader's faster-whisper
    /// fallback) and free its disk space.
    pub async fn remove_model(
        &self,
        app: AppHandle,
        model_id: String,
    ) -> Result<Vec<InstalledModel>, ControllerError> {
        let active_model = self.settings.lock().await.active_model.clone();
        if active_model == model_id {
            return Err(ControllerError::Runtime(
                "Switch to another model before removing this one.".to_string(),
            ));
        }
        let mut manager = self.model_manager.lock().await;
        manager.remove_model_files(&model_id)?;
        self.model_statuses.lock().await.remove(&model_id);
        // Removing the accepted upgrade's model cancels the upgrade, which
        // would otherwise wait for it forever.
        if pending_upgrade().as_deref() == Some(model_id.as_str()) {
            clear_pending_upgrade();
        }
        // A removed faster-whisper fallback is simply no longer the CPU rung:
        // the next dictation re-reads what is installed.
        if let Some(status) = manager.get_download_status(&model_id, Some(active_model.as_str())) {
            self.emit_model_status(&app, &status);
        }
        Ok(manager.list_installed())
    }
}
