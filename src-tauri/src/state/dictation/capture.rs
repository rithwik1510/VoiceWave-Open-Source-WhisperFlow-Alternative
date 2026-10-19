//! Capture: record the utterance (or reuse kept audio on Try again), end
//! quietly on cancel or silence, keep the audio for a retry, and report its
//! quality.

use super::DictationRun;
use crate::audio::{
    analyze_captured_segments,
    input_volume::{self, MicGuardPlan},
    AudioError, AudioQualityBand, AudioQualityReport, CaptureEndPolicy, CaptureOptions,
    HandsFreeLimits, HandsFreeSwitch, VadConfig,
};
use crate::phase1;
use crate::state::{
    clamp_release_tail_ms, clamp_vad_threshold, dictation_retry_instruction,
    effective_release_tail_ms, emit_mic_level, emit_pill_notice, is_likely_low_quality_input_name,
    take_notice_cooldown, ControllerError, DictationLifecycleState, DictationMode, MicLevelEvent,
    RetainedAudio, RetainedCapture, VoiceWaveController, VoiceWaveHudState,
    HANDS_FREE_MAX_CAPTURE_MS, HANDS_FREE_SILENCE_TIMEOUT_MS, MAX_RELEASE_TAIL_MS,
};
use std::{
    sync::{atomic::Ordering, Arc, Mutex as StdMutex},
    time::{Duration, Instant},
};
use tauri::Emitter;
use tokio::sync::oneshot;

pub(super) struct CapturedAudio {
    pub(super) segments: Vec<Vec<f32>>,
    /// The segments joined with 50 ms gaps: what gets decoded, and what is
    /// kept for Try again.
    pub(super) source_samples: Arc<Vec<f32>>,
    pub(super) capture_ms: u64,
    pub(super) captured_audio_ms: u64,
    /// Includes the longer tail for low-quality (headset) mics.
    pub(super) release_tail_ms: u64,
    pub(super) audio_quality: AudioQualityReport,
    /// Mic input volume (percent) read by the volume guard, for telemetry.
    pub(super) mic_input_volume_slot: Arc<StdMutex<Option<u8>>>,
}

impl VoiceWaveController {
    /// `None` means the session already ended (cancelled, or nothing heard)
    /// and the flow stops without an error.
    pub(super) async fn capture_audio(
        &self,
        run: &DictationRun,
    ) -> Result<Option<CapturedAudio>, ControllerError> {
        let DictationRun {
            ref app,
            mode,
            session_id,
            control_mode,
            ref cancel_token,
            ref retry_audio,
            ref settings,
            max_capture_ms,
            ..
        } = *run;
        let mic_input_volume_slot = self.spawn_mic_volume_guard(run);
        let mut release_tail_ms = effective_release_tail_ms(
            clamp_release_tail_ms(settings.release_tail_ms),
            max_capture_ms,
        );
        let capture_started = Instant::now();
        let segments = match (mode, retry_audio.as_ref()) {
            (_, Some(retained)) => vec![retained.samples.to_vec()],
            (DictationMode::Fixture, None) => {
                phase1::build_fixture_segments(settings.vad_threshold)
            }
            (DictationMode::Microphone, None) => {
                let Some(rows) = self.capture_from_mic(run, &mut release_tail_ms).await? else {
                    return Ok(None);
                };
                rows
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
            return Ok(None);
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
            return Ok(None);
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

        let audio_quality = self.assess_capture(run, &segments);

        let total_captured_samples = segments.iter().map(|segment| segment.len()).sum::<usize>();
        let captured_audio_ms =
            ((total_captured_samples as f64 / self.audio.target_sample_rate as f64) * 1000.0)
                .round() as u64;
        Ok(Some(CapturedAudio {
            segments,
            source_samples,
            capture_ms,
            captured_audio_ms,
            release_tail_ms,
            audio_quality,
            mic_input_volume_slot,
        }))
    }

    /// Records from the warm mic (with its pre-roll) until release, the
    /// hands-free second press, or a capture limit.
    async fn capture_from_mic(
        &self,
        run: &DictationRun,
        release_tail_ms: &mut u64,
    ) -> Result<Option<Vec<Vec<f32>>>, ControllerError> {
        let DictationRun {
            ref app,
            session_id,
            control_mode,
            ref cancel_token,
            ref stop_flag,
            ref hands_free,
            flow_started,
            ref settings,
            max_capture_ms,
            whisper_pipeline,
            ..
        } = *run;
        // Hold-to-talk's wait for first speech. Hands-free uses its own long
        // limits (HANDS_FREE_SILENCE_TIMEOUT_MS) via the capture switch.
        let silence_timeout_ms = ((max_capture_ms as f32) * 0.22).round() as u64;
        let silence_timeout_ms = silence_timeout_ms.clamp(700, 2_000);
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
            *release_tail_ms = release_tail_ms.saturating_add(90).min(MAX_RELEASE_TAIL_MS);
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
                release_tail: Duration::from_millis(*release_tail_ms),
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
        let captured = captured_task
            .await
            .map_err(|err| ControllerError::Runtime(format!("audio task join failure: {err}")))?;

        match captured {
            Ok(rows) => Ok(Some(rows)),
            Err(AudioError::Cancelled) => {
                tracing::info!("capture cancelled (session={session_id})");
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
            Err(AudioError::NoSpeechDetected) => {
                tracing::info!("capture ended with no speech (session={session_id})");
                self.set_session_state(session_id, DictationLifecycleState::Idle, None, None)
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
                return Ok(None);
            }
            Err(err) => {
                tracing::error!("capture failed (session={session_id}): {err}");
                return Err(ControllerError::Audio(err));
            }
        }
    }

    /// OS mic-volume guard: external apps (browser auto-gain, call apps,
    /// vendor utilities) silently lower the system capture volume and both
    /// whisper models degrade at once. Fire-and-forget so the COM query
    /// never adds latency to listening start; result lands in the pill as
    /// a Dynamic Island notice and in diagnostics for later triage.
    fn spawn_mic_volume_guard(&self, run: &DictationRun) -> Arc<StdMutex<Option<u8>>> {
        let DictationRun {
            ref app,
            mode,
            ref retry_audio,
            ref settings,
            ..
        } = *run;
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
        mic_input_volume_slot
    }

    /// Analyzes the capture, sends the report to the UI, and warns about poor
    /// audio or a Bluetooth hands-free profile.
    fn assess_capture(&self, run: &DictationRun, segments: &[Vec<f32>]) -> AudioQualityReport {
        let DictationRun {
            ref app,
            mode,
            ref retry_audio,
            ref settings,
            ..
        } = *run;
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
        audio_quality
    }
}
