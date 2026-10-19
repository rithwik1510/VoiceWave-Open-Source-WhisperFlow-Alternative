//! The dictation pipeline: one run from the accepted press to inserted text.

use super::{
    asr_integrity_metrics, clamp_max_utterance_ms, clamp_release_tail_ms, clamp_vad_threshold,
    classify_insertion_target, decode_mode_key, derive_correction_candidates,
    dictation_retry_instruction, effective_release_tail_ms,
    effective_release_watchdog_threshold_ms, emit_mic_level, emit_pill_action_notice,
    emit_pill_notice, emit_pill_rescue, emit_submit_notice, final_decode_timeout,
    floor_decode_mode, fw_min_decode_audio_ms, gpu_step_down_notice, insertion_method_key,
    is_likely_low_quality_input_name, now_utc_ms, release_watchdog_recovered,
    restore_snippet_candidate, return_focus_for_retry, sample_count_to_ms,
    should_reject_low_confidence_transcript_as_no_speech, take_notice_cooldown, ControllerError,
    CorrectionSession, DictationControlMode, DictationLifecycleState, DictationMode,
    LatencyBreakdownEvent, MicLevelEvent, PillAction, RetainedAudio, RetainedCapture,
    TranscriptEvent, VoiceWaveController, VoiceWaveHudState, CORRECTION_SESSION_WINDOW_MS,
    ENGINE_WAIT_NOTICE_GRACE_MS, HANDS_FREE_MAX_CAPTURE_MS, HANDS_FREE_SILENCE_TIMEOUT_MS,
    MAIN_WINDOW_LABEL, MAX_RELEASE_TAIL_MS, PILL_WINDOW_LABEL, POLISH_OUTCOME_ACCEPTED,
    POLISH_OUTCOME_DISABLED, POLISH_OUTCOME_FALLBACK_REJECTED, POLISH_OUTCOME_FALLBACK_TIMEOUT,
    POLISH_OUTCOME_LITERAL, POLISH_OUTCOME_SNIPPET_EXACT, POLISH_PILL_ANNOUNCE_EVERY,
    SUBMIT_DELAY_AFTER_PASTE_MS, SUBMIT_DELAY_AFTER_TYPING_MS, WAIT_VALIDATED_RELEASE_BUDGET_MS,
};
use crate::{
    audio::{
        analyze_captured_segments,
        input_volume::{self, MicGuardPlan},
        AudioError, AudioQualityBand, CaptureEndPolicy, CaptureOptions, HandsFreeLimits,
        HandsFreeSwitch, VadConfig,
    },
    diagnostics::LatencyMetricRecord,
    history::PolishHistoryMeta,
    inference::{
        ensure_faster_whisper_ready, is_faster_whisper_model, is_whisper_cpp_model,
        note_audio_pipeline_decode_hard_failure, note_audio_pipeline_decode_success,
        polish_gate::gate_polish_candidate, uses_whisper_pipeline, warmup_faster_whisper_model,
        InferenceError,
    },
    insertion::InsertTextRequest,
    phase1,
    settings::{resolve_profile, DecodeMode, InsertPath, PolishProfile},
    snippet::{ProtectionError, ProtectionOutcome, SnippetExpansionPlan},
    transcript::{
        finalize_literal_transcript, finalize_pro_transcript, sanitize_user_transcript,
        split_trailing_submit_command, ProTranscriptOptions,
    },
};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex as StdMutex,
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};
use tokio_util::sync::CancellationToken;

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
        let max_capture_ms = clamp_max_utterance_ms(settings.max_utterance_ms);
        let mut release_tail_ms = effective_release_tail_ms(
            clamp_release_tail_ms(settings.release_tail_ms),
            max_capture_ms,
        );
        // Hold-to-talk's wait for first speech. Hands-free uses its own long
        // limits (HANDS_FREE_SILENCE_TIMEOUT_MS) via the capture switch.
        let silence_timeout_ms = ((max_capture_ms as f32) * 0.22).round() as u64;
        let silence_timeout_ms = silence_timeout_ms.clamp(700, 2_000);
        let use_faster_whisper = is_faster_whisper_model(&settings.active_model);
        let use_asr_worker = is_whisper_cpp_model(&settings.active_model);
        // faster-whisper and the whisper.cpp speech worker share capture,
        // conditioning, quality gates and finalization (plan 019).
        let whisper_pipeline = uses_whisper_pipeline(&settings.active_model);
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

        // OS mic-volume guard: external apps (browser auto-gain, call apps,
        // vendor utilities) silently lower the system capture volume and both
        // whisper models degrade at once. Fire-and-forget so the COM query
        // never adds latency to listening start; result lands in the pill as
        // a Dynamic Island notice and in diagnostics for later triage.
        let mic_input_volume_slot: Arc<StdMutex<Option<u8>>> = Arc::new(StdMutex::new(None));
        if mode == DictationMode::Microphone && retry_audio.is_none() {
            let guard_mode = settings.mic_volume_guard;
            let guard_app = app.clone();
            let volume_slot = Arc::clone(&mic_input_volume_slot);
            let last_warned_at = Arc::clone(&self.mic_guard_last_warned_at);
            tauri::async_runtime::spawn(async move {
                let reading = match tokio::task::spawn_blocking(
                    input_volume::read_default_input_volume,
                )
                .await
                {
                    Ok(Ok(reading)) => reading,
                    _ => return,
                };
                if let Ok(mut slot) = volume_slot.lock() {
                    *slot = Some((reading.scalar * 100.0).round().clamp(0.0, 100.0) as u8);
                }
                match input_volume::plan_mic_guard(guard_mode, reading) {
                    MicGuardPlan::Silent => {}
                    MicGuardPlan::WarnMuted => {
                        if take_notice_cooldown(&last_warned_at) {
                            emit_pill_notice(
                                &guard_app,
                                "error",
                                "Microphone is muted in Windows",
                                Some("Unmute it in Sound settings to dictate.".to_string()),
                                6_000,
                            );
                        }
                    }
                    MicGuardPlan::WarnLow { volume_percent } => {
                        if take_notice_cooldown(&last_warned_at) {
                            emit_pill_notice(
                                &guard_app,
                                "warning",
                                format!("Mic input volume is at {volume_percent}%"),
                                Some(
                                    "Another app likely lowered it. Raise it to 100% in Sound settings for best accuracy."
                                        .to_string(),
                                ),
                                6_500,
                            );
                        }
                    }
                    MicGuardPlan::Restore { from_percent } => {
                        let restored =
                            tokio::task::spawn_blocking(input_volume::restore_default_input_volume)
                                .await;
                        match restored {
                            Ok(Ok(())) => {
                                if let Ok(mut slot) = volume_slot.lock() {
                                    *slot = Some(100);
                                }
                                emit_pill_notice(
                                    &guard_app,
                                    "info",
                                    format!("Mic volume restored to 100% (was {from_percent}%)"),
                                    None,
                                    4_500,
                                );
                            }
                            _ => {
                                if take_notice_cooldown(&last_warned_at) {
                                    emit_pill_notice(
                                        &guard_app,
                                        "warning",
                                        format!("Mic input volume is at {from_percent}%"),
                                        Some(
                                            "Auto-restore failed — raise it manually in Sound settings."
                                                .to_string(),
                                        ),
                                        6_500,
                                    );
                                }
                            }
                        }
                    }
                }
            });
        }

        let capture_started = Instant::now();
        let segments = match (mode, retry_audio.as_ref()) {
            (_, Some(retained)) => vec![retained.samples.to_vec()],
            (DictationMode::Fixture, None) => phase1::build_fixture_segments(settings.vad_threshold),
            (DictationMode::Microphone, None) => {
                let audio = self.audio.clone();
                let mic = self.mic.clone();
                // The accepted press, for the mic start-latency log line.
                let pressed_at = self
                    .active_session
                    .lock()
                    .await
                    .as_ref()
                    .map_or(flow_started, |session| session.started_at);
                let input_device = settings.input_device.clone();
                let mut threshold = clamp_vad_threshold(settings.vad_threshold);
                if input_device
                    .as_deref()
                    .is_some_and(is_likely_low_quality_input_name)
                {
                    // Bluetooth/headset mics often lose weak consonants; lower VAD and keep a longer release tail.
                    threshold = clamp_vad_threshold(threshold * 0.82);
                    release_tail_ms = release_tail_ms.saturating_add(90).min(MAX_RELEASE_TAIL_MS);
                }
                let cancel_for_capture = cancel_token.clone();
                let stop_for_capture = stop_flag.clone();
                let capture_options = CaptureOptions {
                    vad_config: VadConfig {
                        threshold,
                        ..VadConfig::default()
                    },
                    max_capture_duration: Duration::from_millis(max_capture_ms),
                    initial_speech_timeout: Duration::from_millis(silence_timeout_ms),
                    // Hold-to-talk ends on release and hands-free on the
                    // second press; while hands-free, the switch below adds
                    // the long safety limits.
                    end_policy: CaptureEndPolicy::ExplicitStop {
                        release_tail: Duration::from_millis(release_tail_ms),
                    },
                    hands_free: Some(HandsFreeSwitch {
                        active: Arc::clone(&hands_free),
                        limits: HandsFreeLimits {
                            max_capture_duration: Duration::from_millis(HANDS_FREE_MAX_CAPTURE_MS),
                            silence_timeout: Duration::from_millis(HANDS_FREE_SILENCE_TIMEOUT_MS),
                        },
                    }),
                    preserve_full_capture: whisper_pipeline,
                    // Adaptive VAD warmup: for the first 2 seconds, use a
                    // lower threshold (60% of normal) so quiet speakers and
                    // mics at distance don't get missed before the system
                    // can detect their voice. After warmup, the full
                    // threshold applies to prevent ambient noise triggers.
                    vad_warmup_threshold: Some(clamp_vad_threshold(threshold * 0.6)),
                    vad_warmup_duration: Duration::from_millis(2_000),
                };
                let app_for_capture = app.clone();
                let mut last_level_emit = Instant::now();
                let (ready_tx, ready_rx) = oneshot::channel::<()>();
                // Clone for the capture closure so the original `input_device`
                // remains available for the no-speech error label further down.
                let input_device_for_capture = input_device.clone();
                let captured_task = tokio::task::spawn_blocking(move || {
                    let mut ready_tx = Some(ready_tx);
                    mic.capture(
                        &audio,
                        input_device_for_capture.as_deref(),
                        capture_options,
                        pressed_at,
                        || cancel_for_capture.is_cancelled(),
                        || stop_for_capture.load(Ordering::Relaxed),
                        move || {
                            if let Some(tx) = ready_tx.take() {
                                let _ = tx.send(());
                            }
                        },
                        move |normalized_chunk, _voiced_chunk| {
                            // 33 ms cadence (~30 fps) so the pill waveform tracks
                            // syllable-level dynamics; 70 ms read as sluggish.
                            if last_level_emit.elapsed() >= Duration::from_millis(33) {
                                let mut peak = 0.0_f32;
                                for sample in normalized_chunk {
                                    peak = peak.max(sample.abs());
                                }
                                emit_mic_level(
                                    &app_for_capture,
                                    MicLevelEvent {
                                        level: peak.clamp(0.0, 1.0),
                                        error: None,
                                    },
                                );
                                last_level_emit = Instant::now();
                            }
                        },
                    )
                });
                if ready_rx.await.is_ok() {
                    // Sound cue is driven from the pill renderer for precise
                    // sync with the visible listening onset.
                    self.update_state(
                        &app,
                        VoiceWaveHudState::Listening,
                        Some("Listening for speech...".to_string()),
                    )
                    .await;
                }
                let captured = captured_task.await.map_err(|err| {
                    ControllerError::Runtime(format!("audio task join failure: {err}"))
                })?;

                match captured {
                    Ok(rows) => rows,
                    Err(AudioError::Cancelled) => {
                        tracing::info!("capture cancelled (session={session_id})");
                        self.set_session_state(
                            session_id,
                            DictationLifecycleState::Idle,
                            None,
                            None,
                        )
                        .await;
                        self.update_state(
                            &app,
                            VoiceWaveHudState::Idle,
                            Some("Dictation cancelled.".to_string()),
                        )
                        .await;
                        return Ok(());
                    }
                    Err(AudioError::NoSpeechDetected) => {
                        tracing::info!("capture ended with no speech (session={session_id})");
                        self.set_session_state(
                            session_id,
                            DictationLifecycleState::Idle,
                            None,
                            None,
                        )
                        .await;
                        let device_label = input_device.as_deref().unwrap_or("system default");
                        let warmup_peak_msg = if threshold > 0.0 {
                            format!(
                                " (warmup threshold {:.4}, full threshold {:.4})",
                                clamp_vad_threshold(threshold * 0.6),
                                threshold
                            )
                        } else {
                            String::new()
                        };
                        self.update_state(
                            &app,
                            VoiceWaveHudState::Idle,
                            Some(format!(
                                "No speech on '{}'{}. {} Try lowering VAD threshold in Settings if this persists.",
                                device_label,
                                warmup_peak_msg,
                                dictation_retry_instruction(control_mode)
                            )),
                        )
                        .await;
                        emit_pill_notice(
                            &app,
                            "info",
                            "Didn't catch that",
                            Some(dictation_retry_instruction(control_mode).to_string()),
                            3_800,
                        );
                        return Ok(());
                    }
                    Err(err) => {
                        tracing::error!("capture failed (session={session_id}): {err}");
                        return Err(ControllerError::Audio(err));
                    }
                }
            }
        };
        let capture_ms = capture_started.elapsed().as_millis() as u64;

        if cancel_token.is_cancelled() {
            self.set_session_state(session_id, DictationLifecycleState::Idle, None, None)
                .await;
            self.update_state(
                &app,
                VoiceWaveHudState::Idle,
                Some("Dictation cancelled.".to_string()),
            )
            .await;
            return Ok(());
        }
        if segments.is_empty() {
            self.set_session_state(session_id, DictationLifecycleState::Idle, None, None)
                .await;
            self.update_state(
                &app,
                VoiceWaveHudState::Idle,
                Some(format!(
                    "No speech captured yet. {}",
                    dictation_retry_instruction(control_mode)
                )),
            )
            .await;
            emit_pill_notice(
                &app,
                "info",
                "Didn't catch that",
                Some(dictation_retry_instruction(control_mode).to_string()),
                3_800,
            );
            return Ok(());
        }

        // Merged right after capture so the audio is kept for a retry before
        // any later step can fail (plan 020).
        let mut merged_samples = Vec::new();
        let inter_segment_gap = (self.audio.target_sample_rate as usize / 20).max(1); // 50 ms gap
        for (idx, segment) in segments.iter().enumerate() {
            if idx > 0 {
                merged_samples.extend(vec![0.0_f32; inter_segment_gap]);
            }
            merged_samples.extend_from_slice(segment);
        }
        let source_samples = Arc::new(merged_samples);
        if mode == DictationMode::Microphone {
            let target_window = match &retry_audio {
                Some(retained) => retained.target_window,
                None => crate::insertion::foreground_window_id(),
            };
            *self.retained_capture.lock().await = Some(RetainedCapture {
                session_id,
                audio: RetainedAudio {
                    samples: Arc::clone(&source_samples),
                    target_window,
                },
            });
        }

        let audio_quality = analyze_captured_segments(
            &segments,
            self.audio.target_sample_rate,
            clamp_vad_threshold(settings.vad_threshold),
        );
        let _ = app.emit("voicewave://audio-quality", audio_quality.clone());

        // A retry already showed these for its original capture.
        if mode == DictationMode::Microphone && retry_audio.is_none() {
            if audio_quality.quality == AudioQualityBand::Poor
                && take_notice_cooldown(&self.poor_audio_notice_last_at)
            {
                emit_pill_notice(
                    &app,
                    "warning",
                    "Audio quality looks poor",
                    Some(
                        "Move closer to the mic, or run the 10s audio check in Settings."
                            .to_string(),
                    ),
                    6_000,
                );
            }
            if settings
                .input_device
                .as_deref()
                .is_some_and(is_likely_low_quality_input_name)
                && !self.mic_profile_notice_sent.swap(true, Ordering::Relaxed)
            {
                emit_pill_notice(
                    &app,
                    "warning",
                    "Headset hands-free profile detected",
                    Some(
                        "Bluetooth call profiles reduce accuracy — the laptop mic array usually transcribes better."
                            .to_string(),
                    ),
                    6_500,
                );
            }
        }

        let total_captured_samples = segments.iter().map(|segment| segment.len()).sum::<usize>();
        let captured_audio_ms =
            ((total_captured_samples as f64 / self.audio.target_sample_rate as f64) * 1000.0)
                .round() as u64;
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
                            &app,
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
                    return Err(ControllerError::Runtime(format!(
                        "Faster-Whisper runtime warmup failed: {err}"
                    )));
                }
                Err(err) => {
                    return Err(ControllerError::Runtime(format!(
                        "Faster-Whisper warmup join failure: {err}"
                    )));
                }
            }
        }
        let final_worker = self
            .build_inference_worker(
                &settings.active_model,
                mode,
                effective_decode_mode,
                fw_runtime_ready,
            )
            .await?;
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
            format!(
                "Transcribing locally (max utterance {}s reached)...",
                capture_cap_ms / 1000
            )
        } else {
            "Transcribing locally...".to_string()
        };
        // Hands-free users may still be talking when the cap stops capture.
        if cap_hit && hands_free_capture {
            emit_pill_notice(
                &app,
                "info",
                format!(
                    "Reached the {}-minute limit",
                    HANDS_FREE_MAX_CAPTURE_MS / 60_000
                ),
                Some("Transcribing what you said so far.".to_string()),
                4_500,
            );
        }

        let transcribing_started = Instant::now();
        let release_to_transcribing_ms = self
            .session_release_elapsed_ms(session_id, transcribing_started)
            .await
            .unwrap_or(0);
        let effective_release_watchdog_ms = effective_release_watchdog_threshold_ms(
            audio_quality.quality,
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
            &app,
            VoiceWaveHudState::Transcribing,
            Some(format!("{transcribing_message}{watchdog_note}")),
        )
        .await;

        let decode_started = Instant::now();
        if mode == DictationMode::Microphone && whisper_pipeline {
            let finalize_audio_ms =
                sample_count_to_ms(source_samples.len(), self.audio.target_sample_rate);
            if finalize_audio_ms < fw_min_decode_audio_ms() {
                self.set_session_state(session_id, DictationLifecycleState::Idle, None, None)
                    .await;
                self.update_state(
                    &app,
                    VoiceWaveHudState::Idle,
                    Some(format!(
                        "Capture was too short to transcribe reliably. {}",
                        dictation_retry_instruction(control_mode)
                    )),
                )
                .await;
                return Ok(());
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
                &app,
                "info",
                "Getting your graphics card ready",
                Some("One-time setup, up to a minute. Your text appears when it's done.".to_string()),
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
                &cancel_token,
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
                    &app,
                    VoiceWaveHudState::Idle,
                    Some("Dictation cancelled.".to_string()),
                )
                .await;
                return Ok(());
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
            if let Some(kind) = decode_telemetry.asr_step_down_kind.filter(|_| {
                !self.gpu_notice_sent.swap(true, Ordering::Relaxed)
            }) {
                let (title, detail) = gpu_step_down_notice(kind, final_worker.active_model());
                emit_pill_notice(&app, "warning", title, Some(detail), 6_500);
            } else if !decode_telemetry.runtime_cache_hit && decode_telemetry.model_init_ms >= 2_500
            {
                emit_pill_notice(
                    &app,
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
            return Ok(());
        }
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
                tracing::warn!("deterministic formatting changed a snippet slot; using protected baseline");
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
                return Ok(());
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
            return Ok(());
        }

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
