//! Transcription: pick the decode mode, wait for the speech engine, enter
//! Transcribing, and decode the whole capture under a timeout (the speech
//! worker's GPU-to-CPU fallback runs inside that timeout).

use super::capture::CapturedAudio;
use super::preflight::FwReadyTask;
use super::DictationRun;
use crate::audio::AudioQualityBand;
use crate::inference::{
    note_audio_pipeline_decode_hard_failure, note_audio_pipeline_decode_success, DecodeTelemetry,
    InferenceError, InferenceWorker, StepDownKind,
};
use crate::settings::DecodeMode;
use crate::state::{
    decode_mode_key, dictation_retry_instruction, effective_release_watchdog_threshold_ms,
    emit_pill_notice, final_decode_timeout, floor_decode_mode, fw_min_decode_audio_ms,
    is_likely_low_quality_input_name, release_watchdog_recovered, sample_count_to_ms,
    ControllerError, DictationLifecycleState, DictationMode, VoiceWaveController,
    VoiceWaveHudState, ENGINE_WAIT_NOTICE_GRACE_MS, HANDS_FREE_MAX_CAPTURE_MS,
};
use crate::transcript::sanitize_user_transcript;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tauri::AppHandle;
use tokio::time::timeout;

pub(super) struct Transcription {
    pub(super) worker: InferenceWorker,
    pub(super) mode_choice: DecodeModeChoice,
    pub(super) start: TranscribingStart,
    pub(super) decoded: Decoded,
}

pub(super) struct DecodeModeChoice {
    /// What the runtime policy picked, before the audio/settings floors.
    pub(super) policy_selected: DecodeMode,
    pub(super) effective: DecodeMode,
    /// `|`-joined trail of the policy and floors that applied.
    pub(super) reason: String,
}

pub(super) struct TranscribingStart {
    pub(super) transcribing_started: Instant,
    pub(super) release_to_transcribing_ms: u64,
    pub(super) effective_release_watchdog_ms: u64,
    pub(super) watchdog_recovered: bool,
    /// A hands-free capture ran into its cap; the pill says so once the
    /// text lands (`announce_insertion`).
    pub(super) hands_free_cap_hit: bool,
}

pub(super) struct Decoded {
    pub(super) telemetry: DecodeTelemetry,
    /// Sanitized ASR text, before commands or formatting.
    pub(super) finalize_text: String,
    pub(super) release_finalize_ms: u64,
    pub(super) decode_ms: u64,
    pub(super) finalize_tail_audio_ms: u64,
}

impl VoiceWaveController {
    /// `None` means the session already ended (capture too short, or
    /// cancelled mid-decode) and the flow stops without an error.
    pub(super) async fn transcribe(
        &self,
        run: &DictationRun,
        capture: &CapturedAudio,
        fw_ready_task: Option<FwReadyTask>,
    ) -> Result<Option<Transcription>, ControllerError> {
        let mode_choice = self.select_decode_mode(run, capture).await;
        let fw_runtime_ready = wait_for_faster_whisper(&run.app, fw_ready_task).await?;
        let worker = self
            .build_inference_worker(
                &run.settings.active_model,
                run.mode,
                mode_choice.effective,
                fw_runtime_ready,
            )
            .await?;
        let start = self.enter_transcribing(run, capture).await;
        let Some(decoded) = self.decode_capture(run, &worker, capture).await? else {
            return Ok(None);
        };
        Ok(Some(Transcription {
            worker,
            mode_choice,
            start,
            decoded,
        }))
    }

    async fn select_decode_mode(
        &self,
        run: &DictationRun,
        capture: &CapturedAudio,
    ) -> DecodeModeChoice {
        let DictationRun {
            mode,
            ref settings,
            whisper_pipeline,
            ..
        } = *run;
        let CapturedAudio {
            captured_audio_ms,
            ref audio_quality,
            ..
        } = *capture;
        let policy_selected_mode = match mode {
            DictationMode::Fixture => settings.decode_mode,
            DictationMode::Microphone => self
                .decode_policy
                .lock()
                .await
                .select_mode(captured_audio_ms),
        };
        let mut effective_decode_mode = policy_selected_mode;
        let mut decode_policy_reasons = vec![match mode {
            DictationMode::Fixture => "fixture-settings".to_string(),
            DictationMode::Microphone => {
                format!("runtime-policy:{}", decode_mode_key(policy_selected_mode))
            }
        }];
        if whisper_pipeline && mode == DictationMode::Microphone {
            let mut decode_floor = match audio_quality.quality {
                AudioQualityBand::Good => DecodeMode::Fast,
                AudioQualityBand::Fair => DecodeMode::Balanced,
                AudioQualityBand::Poor => DecodeMode::Quality,
            };
            if settings
                .input_device
                .as_deref()
                .is_some_and(is_likely_low_quality_input_name)
            {
                decode_floor = floor_decode_mode(decode_floor, DecodeMode::Balanced);
            }
            let pre_floor = effective_decode_mode;
            effective_decode_mode = floor_decode_mode(effective_decode_mode, decode_floor);
            if effective_decode_mode != pre_floor {
                decode_policy_reasons
                    .push(format!("fw-audio-floor:{}", decode_mode_key(decode_floor)));
            }
            // The fast lanes that previously downgraded Balanced -> Fast here
            // (clean-audio floor and short-utterance) are removed: beam=1
            // greedy output regularly passed the confidence retry gate while
            // being wrong, so short dictations shipped degraded text. Beam=5
            // is ~0.3 s on GPU and the latency policy still handles genuinely
            // slow machines.
        }
        if mode == DictationMode::Microphone {
            // The user's decode-mode setting is a floor for live dictation, so
            // "balanced"/"quality" in settings can never be silently downgraded
            // by the runtime policy. "fast" imposes no floor and lets the
            // policy manage the speed/quality trade-off.
            let pre_floor = effective_decode_mode;
            effective_decode_mode = floor_decode_mode(effective_decode_mode, settings.decode_mode);
            if effective_decode_mode != pre_floor {
                decode_policy_reasons.push(format!(
                    "settings-floor:{}",
                    decode_mode_key(settings.decode_mode)
                ));
            }
        }
        let decode_policy_reason = decode_policy_reasons.join("|");
        DecodeModeChoice {
            policy_selected: policy_selected_mode,
            effective: effective_decode_mode,
            reason: decode_policy_reason,
        }
    }

    /// Shows the cap-hit message/notice, measures release-to-Transcribing
    /// (and whether the release watchdog had to recover), and moves the
    /// session and HUD to Transcribing.
    async fn enter_transcribing(
        &self,
        run: &DictationRun,
        capture: &CapturedAudio,
    ) -> TranscribingStart {
        let DictationRun {
            ref app,
            mode,
            session_id,
            ref stop_flag,
            ref hands_free,
            ref retry_audio,
            max_capture_ms,
            ..
        } = *run;
        let CapturedAudio {
            captured_audio_ms,
            release_tail_ms,
            ref audio_quality,
            ..
        } = *capture;
        let hands_free_capture = hands_free.load(Ordering::Relaxed);
        let capture_cap_ms = if hands_free_capture {
            HANDS_FREE_MAX_CAPTURE_MS
        } else {
            max_capture_ms
        };
        let cap_hit = mode == DictationMode::Microphone
            && retry_audio.is_none()
            && !stop_flag.load(Ordering::Relaxed)
            && captured_audio_ms.saturating_add(150) >= capture_cap_ms;
        let transcribing_message = if cap_hit {
            let limit = if capture_cap_ms % 60_000 == 0 {
                format!("{}-minute", capture_cap_ms / 60_000)
            } else {
                format!("{}-second", capture_cap_ms / 1000)
            };
            format!("Transcribing locally (reached the {limit} limit)...")
        } else {
            "Transcribing locally...".to_string()
        };

        let transcribing_started = Instant::now();
        let release_to_transcribing_ms = self
            .session_release_elapsed_ms(session_id, transcribing_started)
            .await
            .unwrap_or(0);
        let effective_release_watchdog_ms = effective_release_watchdog_threshold_ms(
            audio_quality.quality.clone(),
            captured_audio_ms,
            release_tail_ms,
        );
        let watchdog_recovered =
            release_watchdog_recovered(release_to_transcribing_ms, effective_release_watchdog_ms);
        if watchdog_recovered {
            self.watchdog_recovery_count.fetch_add(1, Ordering::Relaxed);
        }
        let watchdog_note = if watchdog_recovered {
            " (release watchdog recovered delayed transition)"
        } else {
            ""
        };
        self.set_session_state(
            session_id,
            DictationLifecycleState::Transcribing,
            None,
            None,
        )
        .await;
        self.update_state(
            app,
            VoiceWaveHudState::Transcribing,
            Some(format!("{transcribing_message}{watchdog_note}")),
        )
        .await;
        TranscribingStart {
            transcribing_started,
            release_to_transcribing_ms,
            effective_release_watchdog_ms,
            watchdog_recovered,
            hands_free_cap_hit: cap_hit && hands_free_capture,
        }
    }

    /// Decodes the whole capture. Failures and timeouts feed the decode
    /// policy; a missing model is reported as setup, not a decode failure.
    async fn decode_capture(
        &self,
        run: &DictationRun,
        final_worker: &InferenceWorker,
        capture: &CapturedAudio,
    ) -> Result<Option<Decoded>, ControllerError> {
        let DictationRun {
            ref app,
            mode,
            session_id,
            control_mode,
            ref cancel_token,
            whisper_pipeline,
            ..
        } = *run;
        let CapturedAudio {
            ref source_samples, ..
        } = *capture;
        let decode_started = Instant::now();
        if mode == DictationMode::Microphone && whisper_pipeline {
            let finalize_audio_ms =
                sample_count_to_ms(source_samples.len(), self.audio.target_sample_rate);
            if finalize_audio_ms < fw_min_decode_audio_ms() {
                self.set_session_state(session_id, DictationLifecycleState::Idle, None, None)
                    .await;
                self.update_state(
                    app,
                    VoiceWaveHudState::Idle,
                    Some(format!(
                        "Capture was too short to transcribe reliably. {}",
                        dictation_retry_instruction(control_mode)
                    )),
                )
                .await;
                return Ok(None);
            }
        }
        // The whole capture is decoded at release.
        let finalize_tail_audio_ms =
            sample_count_to_ms(source_samples.len(), self.audio.target_sample_rate);
        let finalize_started = Instant::now();
        let finalize_samples_ref: &[f32] = source_samples.as_slice();
        let default_finalize_timeout =
            final_decode_timeout(finalize_samples_ref.len(), self.audio.target_sample_rate);
        // During the one-time GPU check, a whisper.cpp model with no
        // faster-whisper fallback waits for the check before decoding.
        let waits_for_gpu_check = final_worker.waits_for_gpu_check().await;
        // The speech worker may fail on the GPU and retry the same audio on
        // the CPU rung; the outer timeout covers that wait and both attempts.
        let finalize_timeout = final_worker
            .asr_worker_timeout(
                sample_count_to_ms(finalize_samples_ref.len(), self.audio.target_sample_rate),
                default_finalize_timeout,
                waits_for_gpu_check,
            )
            .unwrap_or(default_finalize_timeout);
        // That wait can take most of a minute, so say why now rather than
        // after the result. It uses the session's one GPU notice.
        let gpu_wait_notice = waits_for_gpu_check
            && mode == DictationMode::Microphone
            && !self.gpu_notice_sent.swap(true, Ordering::Relaxed);
        if gpu_wait_notice {
            emit_pill_notice(
                app,
                "info",
                "Getting your graphics card ready",
                Some(
                    "One-time setup, up to a minute. Your text appears when it's done.".to_string(),
                ),
                15_000,
            );
        }
        // Long decodes leave the user staring at a spinner with no promise it
        // is alive; past ~2.5s the pill says so explicitly (unless the GPU
        // notice above already explains the wait).
        let decode_watch_done = Arc::new(AtomicBool::new(false));
        if mode == DictationMode::Microphone && !gpu_wait_notice {
            let done = Arc::clone(&decode_watch_done);
            let watch_app = app.clone();
            let watch_cancel = cancel_token.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(Duration::from_millis(2_500)).await;
                if !done.load(Ordering::Relaxed) && !watch_cancel.is_cancelled() {
                    emit_pill_notice(
                        &watch_app,
                        "info",
                        "Still transcribing…",
                        Some("Long dictations take a little more time.".to_string()),
                        2_600,
                    );
                }
            });
        }
        let finalize_result = timeout(
            finalize_timeout,
            final_worker.transcribe_segment(
                finalize_samples_ref,
                cancel_token,
                |_text, _is_final, _elapsed_ms| {},
            ),
        )
        .await;
        decode_watch_done.store(true, Ordering::Relaxed);
        let finalize_output = match finalize_result {
            Ok(Ok(text)) => text,
            Ok(Err(InferenceError::Cancelled)) => {
                self.set_session_state(session_id, DictationLifecycleState::Idle, None, None)
                    .await;
                self.update_state(
                    app,
                    VoiceWaveHudState::Idle,
                    Some("Dictation cancelled.".to_string()),
                )
                .await;
                return Ok(None);
            }
            Ok(Err(err)) => {
                // The worker refuses to load a model that is not on disk yet
                // (worker.py raises "... is not downloaded yet"). That is a
                // setup state, not a decode failure: feeding it to the policy
                // or the audio-pipeline fallback counters would degrade later
                // dictations once the download finishes.
                // The pill notice comes from `dictation_failure_notice`.
                if err.to_string().contains("is not downloaded yet") {
                    return Err(ControllerError::MissingModel(
                        final_worker.active_model().to_string(),
                    ));
                }
                // Likewise a speech worker that never came up (executable
                // missing or quarantined, launch failed): a setup fault, so
                // no decode-failure counting and no Try again.
                if matches!(
                    err,
                    InferenceError::AsrWorker {
                        failed_start: true,
                        ..
                    } | InferenceError::WorkerStartFailed(_)
                ) {
                    return Err(ControllerError::EngineStart(format!(
                        "Speech engine failed to start for model '{}': {err}",
                        final_worker.active_model()
                    )));
                }
                if mode == DictationMode::Microphone {
                    self.decode_policy
                        .lock()
                        .await
                        .record_failure(finalize_started.elapsed().as_millis() as u64);
                    if whisper_pipeline {
                        note_audio_pipeline_decode_hard_failure();
                    }
                }
                return Err(ControllerError::Runtime(format!(
                    "Inference failed for model '{}': {err}",
                    final_worker.active_model()
                )));
            }
            Err(_) => {
                cancel_token.cancel();
                if mode == DictationMode::Microphone {
                    self.decode_policy
                        .lock()
                        .await
                        .record_failure(finalize_started.elapsed().as_millis() as u64);
                    if whisper_pipeline {
                        note_audio_pipeline_decode_hard_failure();
                    }
                }
                return Err(ControllerError::Runtime(format!(
                    "Inference timed out after {} ms for model '{}'.",
                    finalize_timeout.as_millis(),
                    final_worker.active_model()
                )));
            }
        };
        let release_finalize_ms = finalize_started.elapsed().as_millis() as u64;
        let decode_ms = decode_started.elapsed().as_millis() as u64;
        let decode_telemetry = finalize_output.telemetry;
        if mode == DictationMode::Microphone && whisper_pipeline {
            note_audio_pipeline_decode_success(&decode_telemetry.audio_pipeline_version);
        }
        if mode == DictationMode::Microphone {
            if let Some(kind) = decode_telemetry
                .asr_step_down_kind
                .filter(|_| !self.gpu_notice_sent.swap(true, Ordering::Relaxed))
            {
                let (title, detail) = gpu_step_down_notice(kind, final_worker.active_model());
                emit_pill_notice(app, "warning", title, Some(detail), 6_500);
            } else if !decode_telemetry.runtime_cache_hit && decode_telemetry.model_init_ms >= 2_500
            {
                emit_pill_notice(
                    app,
                    "info",
                    "Engine was cold",
                    Some(
                        "First dictation after a restart takes longer — the next ones are fast."
                            .to_string(),
                    ),
                    4_500,
                );
            }
        }
        let raw_decode_transcript = finalize_output.transcript.unwrap_or_default();
        let finalize_text = sanitize_user_transcript(&raw_decode_transcript);
        Ok(Some(Decoded {
            telemetry: decode_telemetry,
            finalize_text,
            release_finalize_ms,
            decode_ms,
            finalize_tail_audio_ms,
        }))
    }
}

/// Joins the faster-whisper readiness task. A wait past the grace period
/// gets a "Loading the speech model" notice. Returns whether the runtime is
/// ready.
async fn wait_for_faster_whisper(
    app: &AppHandle,
    fw_ready_task: Option<FwReadyTask>,
) -> Result<bool, ControllerError> {
    let mut fw_runtime_ready = false;
    if let Some(mut task) = fw_ready_task {
        // Capture is done, but the engine may still be loading the model:
        // dictating seconds after launch waits on the startup prefetch, and
        // a crashed worker reloads in the background. That wait is real
        // (no decode can start before the model is in memory) and used to
        // be silent, so say what is happening. Releases that land on a
        // warm engine never see this.
        let grace = Duration::from_millis(ENGINE_WAIT_NOTICE_GRACE_MS);
        let joined = if task.is_finished() {
            task.await
        } else {
            match timeout(grace, &mut task).await {
                Ok(joined) => joined,
                Err(_) => {
                    emit_pill_notice(
                        app,
                        "info",
                        "Loading the speech model",
                        Some(
                            "Your words are captured. Transcription starts as soon as it's ready."
                                .to_string(),
                        ),
                        5_000,
                    );
                    task.await
                }
            }
        };
        match joined {
            Ok(Ok(())) => {
                fw_runtime_ready = true;
            }
            Ok(Err(err)) => {
                return Err(ControllerError::EngineStart(format!(
                    "Faster-Whisper runtime warmup failed: {err}"
                )));
            }
            Err(err) => {
                return Err(ControllerError::EngineStart(format!(
                    "Faster-Whisper warmup join failure: {err}"
                )));
            }
        }
    }
    Ok(fw_runtime_ready)
}

/// Copy for the once-per-session GPU step-down pill (plan 019). A Large
/// Turbo dictation on the processor can take ~10 s, so say why, and point at
/// Small, which is fast there. `Preparing` normally arrives here when a
/// faster-whisper model decoded during the GPU check; without one the
/// dictation waits for the check and its notice goes out up front.
fn gpu_step_down_notice(kind: StepDownKind, model_id: &str) -> (&'static str, String) {
    let turbo = model_id.contains("large-v3-turbo");
    match kind {
        StepDownKind::Preparing => (
            "Getting your graphics card ready",
            "One-time setup, up to a minute. Until then dictation runs on your processor."
                .to_string(),
        ),
        StepDownKind::Unavailable | StepDownKind::Fault => (
            "Ran on your processor this time",
            if turbo {
                "Your graphics card wasn't available, so this took longer. Small is much faster on the processor; switch in Models."
            } else {
                "Your graphics card wasn't available, so this one took a little longer."
            }
            .to_string(),
        ),
    }
}
