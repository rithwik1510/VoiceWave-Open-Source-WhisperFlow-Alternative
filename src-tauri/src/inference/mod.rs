mod audio_pipeline;
mod backend;
mod executor;
mod faster_whisper;
pub mod llm_polish;
pub mod polish_gate;
mod policy;

pub use executor::{cpu_runtime_pool_enabled, prewarm_runtime};
pub use faster_whisper::{
    cache_hint_for_model as faster_whisper_cache_hint, ensure_faster_whisper_ready,
    kill_worker_processes as kill_faster_whisper_workers,
};
pub use policy::RuntimeDecodePolicy;

use crate::settings::DecodeMode;
use backend::{
    context_params_for_backend, note_gpu_runtime_failure, preferred_backend_for_model,
    RuntimeBackend,
};
use directories::ProjectDirs;
use faster_whisper::FasterWhisperRequestOverrides;
use std::{
    collections::HashSet,
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use tokio::time::{sleep, Duration};
use tokio_util::sync::CancellationToken;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext};

#[derive(Debug, Clone, Default)]
pub struct DecodeTelemetry {
    pub model_init_ms: u64,
    pub audio_condition_ms: u64,
    pub decode_compute_ms: u64,
    pub audio_pipeline_version: String,
    pub runtime_cache_hit: bool,
    pub backend_requested: String,
    pub backend_used: String,
    pub backend_fallback: bool,
    pub fw_segment_count: Option<u32>,
    pub fw_avg_logprob: Option<f32>,
    pub fw_no_speech_prob: Option<f32>,
    pub fw_compression_ratio: Option<f32>,
    pub fw_low_coherence: bool,
    pub fw_retry_used: bool,
    pub fw_literal_retry_used: bool,
    pub fw_shadow_candidate_version: Option<String>,
    pub fw_shadow_quality_delta: Option<f32>,
    pub fw_shadow_candidate_avg_logprob: Option<f32>,
    pub fw_shadow_candidate_no_speech_prob: Option<f32>,
    pub fw_shadow_candidate_retry_used: Option<bool>,
    pub fw_shadow_candidate_low_coherence: Option<bool>,
    pub fw_shadow_candidate_decode_compute_ms: Option<u64>,
    pub fw_shadow_candidate_won: Option<bool>,
    pub audio_pipeline_fallback_engaged: bool,
    pub audio_pipeline_fallback_remaining: u32,
}

fn debug_fw_gpu_log(hypothesis_id: &str, location: &str, message: &str, kvs: &[(&str, String)]) {
    if !fw_debug_logs_enabled() {
        return;
    }

    let Some(log_path) = fw_debug_log_path() else {
        return;
    };

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let session_id = std::env::var("VOICEWAVE_FW_DEBUG_SESSION_ID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "local".to_string());
    let run_id = std::env::var("VOICEWAVE_FW_DEBUG_RUN_ID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "gpu-debug".to_string());

    let mut data = serde_json::Map::new();
    for (key, value) in kvs {
        data.insert((*key).to_string(), serde_json::Value::String(value.clone()));
    }

    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let payload = serde_json::json!({
        "sessionId": session_id,
        "runId": run_id,
        "hypothesisId": hypothesis_id,
        "location": location,
        "message": message,
        "data": data,
        "timestamp": ts
    });

    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
    {
        let _ = writeln!(file, "{}", payload);
    }
}

fn fw_debug_logs_enabled() -> bool {
    std::env::var("VOICEWAVE_ENABLE_FW_DEBUG_LOGS")
        .map(|value| matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
}

fn fw_debug_log_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("VOICEWAVE_FW_DEBUG_LOG_PATH") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
    }

    ProjectDirs::from("com", "voicewave", "localcore")
        .map(|dirs| dirs.data_dir().join("logs").join("fw-gpu-debug.log"))
}

#[derive(Debug, Clone)]
pub struct SegmentTranscription {
    pub transcript: Option<String>,
    pub telemetry: DecodeTelemetry,
}

#[derive(Debug, Clone, Copy)]
pub enum DecodeStrategy {
    Greedy { best_of: i32 },
    BeamSearch { beam_size: i32, patience: f32 },
}

#[derive(Debug, Clone, Copy)]
pub struct ModelDecodeProfile {
    pub partial_delay_ms: u64,
    pub strategy: DecodeStrategy,
    pub no_speech_thold: f32,
    pub thread_cap: usize,
    pub no_context: bool,
}

pub fn decode_profile(model_id: &str) -> ModelDecodeProfile {
    decode_profile_for_mode(model_id, DecodeMode::Balanced)
}

pub fn decode_profile_for_mode(model_id: &str, decode_mode: DecodeMode) -> ModelDecodeProfile {
    let balanced = match model_id {
        "fw-small.en" => ModelDecodeProfile {
            partial_delay_ms: 70,
            strategy: DecodeStrategy::BeamSearch {
                beam_size: 2,
                patience: -1.0,
            },
            no_speech_thold: 0.5,
            thread_cap: 12,
            no_context: false,
        },
        "fw-large-v3" => ModelDecodeProfile {
            partial_delay_ms: 120,
            strategy: DecodeStrategy::BeamSearch {
                beam_size: 3,
                patience: -1.0,
            },
            no_speech_thold: 0.45,
            thread_cap: 12,
            no_context: false,
        },
        "fw-large-v3-turbo" => ModelDecodeProfile {
            partial_delay_ms: 90,
            strategy: DecodeStrategy::BeamSearch {
                beam_size: 3,
                patience: -1.0,
            },
            no_speech_thold: 0.45,
            thread_cap: 12,
            no_context: false,
        },
        "wcpp-small.en" => ModelDecodeProfile {
            partial_delay_ms: 70,
            strategy: DecodeStrategy::BeamSearch {
                beam_size: 2,
                patience: -1.0,
            },
            no_speech_thold: 0.5,
            thread_cap: 12,
            no_context: false,
        },
        "wcpp-large-v3-turbo" => ModelDecodeProfile {
            partial_delay_ms: 110,
            strategy: DecodeStrategy::BeamSearch {
                beam_size: 3,
                patience: -1.0,
            },
            no_speech_thold: 0.45,
            thread_cap: 12,
            no_context: false,
        },
        "tiny.en" => ModelDecodeProfile {
            partial_delay_ms: 60,
            strategy: DecodeStrategy::Greedy { best_of: 2 },
            no_speech_thold: 0.55,
            thread_cap: 8,
            no_context: false,
        },
        "base.en" => ModelDecodeProfile {
            partial_delay_ms: 85,
            strategy: DecodeStrategy::Greedy { best_of: 2 },
            no_speech_thold: 0.5,
            thread_cap: 8,
            no_context: false,
        },
        "small.en" => ModelDecodeProfile {
            partial_delay_ms: 95,
            strategy: DecodeStrategy::BeamSearch {
                beam_size: 2,
                patience: -1.0,
            },
            no_speech_thold: 0.45,
            thread_cap: 12,
            no_context: false,
        },
        "medium.en" => ModelDecodeProfile {
            partial_delay_ms: 150,
            strategy: DecodeStrategy::BeamSearch {
                beam_size: 5,
                patience: -1.0,
            },
            no_speech_thold: 0.4,
            thread_cap: 8,
            no_context: false,
        },
        _ => ModelDecodeProfile {
            partial_delay_ms: 120,
            strategy: DecodeStrategy::Greedy { best_of: 2 },
            no_speech_thold: 0.5,
            thread_cap: 8,
            no_context: false,
        },
    };

    match decode_mode {
        DecodeMode::Balanced => balanced,
        DecodeMode::Fast => ModelDecodeProfile {
            partial_delay_ms: balanced.partial_delay_ms.saturating_sub(20).max(20),
            strategy: DecodeStrategy::Greedy { best_of: 1 },
            no_speech_thold: (balanced.no_speech_thold + 0.05).clamp(0.1, 0.9),
            thread_cap: 12,
            no_context: true,
        },
        DecodeMode::Quality => {
            let strategy = match balanced.strategy {
                DecodeStrategy::Greedy { .. } => DecodeStrategy::Greedy { best_of: 3 },
                DecodeStrategy::BeamSearch { beam_size, .. } => DecodeStrategy::BeamSearch {
                    beam_size: (beam_size + 1).min(6),
                    patience: -1.0,
                },
            };
            ModelDecodeProfile {
                partial_delay_ms: balanced.partial_delay_ms + 25,
                strategy,
                no_speech_thold: (balanced.no_speech_thold - 0.05).clamp(0.1, 0.9),
                thread_cap: 8,
                no_context: false,
            }
        }
    }
}

pub fn decode_profile_from_str(model_id: &str, decode_mode: &str) -> ModelDecodeProfile {
    let mode = match decode_mode.to_ascii_lowercase().as_str() {
        "fast" => DecodeMode::Fast,
        "quality" => DecodeMode::Quality,
        _ => DecodeMode::Balanced,
    };
    decode_profile_for_mode(model_id, mode)
}

#[derive(Debug, thiserror::Error)]
pub enum InferenceError {
    #[error("active model file is missing at path: {path}")]
    ModelFileMissing { path: String },
    #[error("active model file is not a GGUF or BIN artifact: {path}")]
    ModelFormatUnsupported { path: String },
    #[error("failed to load whisper model '{model_id}' from '{path}': {reason}")]
    ModelLoadFailed {
        model_id: String,
        path: String,
        reason: String,
    },
    #[error("failed to create whisper runtime state: {0}")]
    StateInitFailed(String),
    #[error("whisper decode failed for model '{model_id}': {reason}")]
    DecodeFailed { model_id: String, reason: String },
    #[error("decode cancelled")]
    Cancelled,
    #[error("decode task join failed: {0}")]
    RuntimeJoin(String),
}

#[derive(Debug, Clone)]
enum InferenceBackend {
    Whisper {
        model_path: PathBuf,
        decode_mode: DecodeMode,
    },
    FasterWhisper {
        decode_mode: DecodeMode,
        terminology_hint: Option<String>,
    },
    Scripted,
}

#[derive(Debug, Clone)]
pub struct InferenceWorker {
    active_model: String,
    scripted_output: Option<String>,
    partial_delay_ms: u64,
    decode_mode: DecodeMode,
    backend: InferenceBackend,
}

impl InferenceWorker {
    pub fn new_runtime(active_model: impl Into<String>, model_path: impl Into<PathBuf>) -> Self {
        Self::new_runtime_with_mode(active_model, model_path, DecodeMode::Balanced)
    }

    pub fn new_runtime_with_mode(
        active_model: impl Into<String>,
        model_path: impl Into<PathBuf>,
        decode_mode: DecodeMode,
    ) -> Self {
        let active_model = active_model.into();
        let profile = decode_profile_for_mode(&active_model, decode_mode);
        Self {
            active_model,
            scripted_output: None,
            partial_delay_ms: profile.partial_delay_ms,
            decode_mode,
            backend: InferenceBackend::Whisper {
                model_path: model_path.into(),
                decode_mode,
            },
        }
    }

    pub fn new_fixture(active_model: impl Into<String>) -> Self {
        let active_model = active_model.into();
        let profile = decode_profile_for_mode(&active_model, DecodeMode::Balanced);
        Self {
            active_model,
            scripted_output: None,
            partial_delay_ms: profile.partial_delay_ms,
            decode_mode: DecodeMode::Balanced,
            backend: InferenceBackend::Scripted,
        }
    }

    pub fn new_faster_whisper(active_model: impl Into<String>) -> Self {
        Self::new_faster_whisper_with_mode(active_model, DecodeMode::Balanced)
    }

    pub fn new_faster_whisper_with_mode(
        active_model: impl Into<String>,
        decode_mode: DecodeMode,
    ) -> Self {
        Self::new_faster_whisper_with_mode_and_hint(active_model, decode_mode, None)
    }

    pub fn new_faster_whisper_with_mode_and_hint(
        active_model: impl Into<String>,
        decode_mode: DecodeMode,
        terminology_hint: Option<String>,
    ) -> Self {
        let active_model = active_model.into();
        let profile = decode_profile_for_mode(&active_model, decode_mode);
        let hint = terminology_hint
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        Self {
            active_model,
            scripted_output: None,
            partial_delay_ms: profile.partial_delay_ms,
            decode_mode,
            backend: InferenceBackend::FasterWhisper {
                decode_mode,
                terminology_hint: hint,
            },
        }
    }

    pub fn with_script(
        active_model: impl Into<String>,
        scripted_output: impl Into<String>,
    ) -> Self {
        let active_model = active_model.into();
        let profile = decode_profile_for_mode(&active_model, DecodeMode::Balanced);
        Self {
            active_model,
            scripted_output: Some(scripted_output.into()),
            partial_delay_ms: profile.partial_delay_ms,
            decode_mode: DecodeMode::Balanced,
            backend: InferenceBackend::Scripted,
        }
    }

    pub fn active_model(&self) -> &str {
        &self.active_model
    }

    pub fn decode_mode(&self) -> DecodeMode {
        self.decode_mode
    }

    pub fn model_path(&self) -> Option<&Path> {
        match &self.backend {
            InferenceBackend::Whisper { model_path, .. } => Some(model_path.as_path()),
            InferenceBackend::FasterWhisper { .. } => None,
            InferenceBackend::Scripted => None,
        }
    }

    pub fn with_partial_delay_ms(mut self, delay_ms: u64) -> Self {
        self.partial_delay_ms = delay_ms;
        self
    }

    pub async fn transcribe_segment<F>(
        &self,
        samples: &[f32],
        cancel_token: &CancellationToken,
        mut on_event: F,
    ) -> Result<SegmentTranscription, InferenceError>
    where
        F: FnMut(&str, bool, u64),
    {
        if samples.is_empty() {
            return Ok(SegmentTranscription {
                transcript: None,
                telemetry: DecodeTelemetry::default(),
            });
        }

        let started = Instant::now();
        let maybe_text = match &self.backend {
            InferenceBackend::Scripted => {
                let text = self.mock_decode_output(samples.len());
                SegmentTranscription {
                    transcript: Some(text),
                    telemetry: DecodeTelemetry::default(),
                }
            }
            InferenceBackend::Whisper {
                model_path,
                decode_mode,
            } => {
                let decode = transcribe_with_whisper(
                    samples.to_vec(),
                    self.active_model.clone(),
                    model_path.clone(),
                    *decode_mode,
                    cancel_token.clone(),
                )
                .await?;
                SegmentTranscription {
                    transcript: Some(decode.text),
                    telemetry: decode.telemetry,
                }
            }
            InferenceBackend::FasterWhisper {
                decode_mode,
                terminology_hint,
            } => {
                let decode = transcribe_with_faster_whisper(
                    samples.to_vec(),
                    self.active_model.clone(),
                    *decode_mode,
                    terminology_hint.clone(),
                    cancel_token.clone(),
                )
                .await?;
                SegmentTranscription {
                    transcript: Some(decode.text),
                    telemetry: decode.telemetry,
                }
            }
        };

        let Some(text) = maybe_text.transcript.clone() else {
            return Ok(SegmentTranscription {
                transcript: None,
                telemetry: maybe_text.telemetry,
            });
        };
        if text.trim().is_empty() || cancel_token.is_cancelled() {
            return Ok(SegmentTranscription {
                transcript: None,
                telemetry: maybe_text.telemetry,
            });
        }

        if matches!(
            self.backend,
            InferenceBackend::Whisper { .. } | InferenceBackend::FasterWhisper { .. }
        ) {
            // Runtime decode is already complete here; avoid synthetic per-word delay.
            on_event(&text, true, started.elapsed().as_millis() as u64);
            return Ok(SegmentTranscription {
                transcript: Some(text),
                telemetry: maybe_text.telemetry,
            });
        }

        let words = text.split_whitespace().collect::<Vec<_>>();
        let mut running_text = String::new();
        for word in &words {
            if cancel_token.is_cancelled() {
                return Ok(SegmentTranscription {
                    transcript: None,
                    telemetry: DecodeTelemetry::default(),
                });
            }
            if !running_text.is_empty() {
                running_text.push(' ');
            }
            running_text.push_str(word);
            on_event(&running_text, false, started.elapsed().as_millis() as u64);
            if self.partial_delay_ms > 0 {
                sleep(Duration::from_millis(self.partial_delay_ms)).await;
            }
        }

        if cancel_token.is_cancelled() {
            return Ok(SegmentTranscription {
                transcript: None,
                telemetry: DecodeTelemetry::default(),
            });
        }

        on_event(&text, true, started.elapsed().as_millis() as u64);
        Ok(SegmentTranscription {
            transcript: Some(text),
            telemetry: DecodeTelemetry::default(),
        })
    }

    fn mock_decode_output(&self, sample_count: usize) -> String {
        if let Some(scripted) = &self.scripted_output {
            return scripted.clone();
        }

        let seconds = sample_count as f32 / 16_000.0;
        if seconds < 1.0 {
            "quick local transcript".to_string()
        } else if seconds < 3.0 {
            "phase one foundation is ready".to_string()
        } else {
            "voicewave phase one local transcript pipeline is stable".to_string()
        }
    }
}

pub fn estimate_rtf(elapsed_ms: u64, sample_count: usize) -> f32 {
    let audio_duration_s = sample_count as f32 / 16_000.0;
    if audio_duration_s <= 0.0 {
        return 0.0;
    }
    (elapsed_ms as f32 / 1000.0) / audio_duration_s
}

pub fn is_faster_whisper_model(model_id: &str) -> bool {
    faster_whisper_runtime_model_id(model_id).is_some()
}

pub fn is_whisper_cpp_model(model_id: &str) -> bool {
    model_id.starts_with("wcpp-")
}

/// Classification of how a given active model should be warmed up at app
/// startup so the first transcription doesn't pay a cold-start penalty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarmupPlan {
    /// Spawn the Python faster-whisper worker and call `prefetch_model` to
    /// load the weights into the worker's MODEL_CACHE. After this, the first
    /// real transcription hits the cache path (~500 ms instead of 2-5 s).
    FasterWhisper,
    /// whisper.cpp backend: load the ggml weights via `prewarm_runtime` so
    /// the first real transcription uses the pre-initialized WhisperContext.
    WhisperCpp,
    /// Model ID we don't know how to warm up. Skip silently.
    Unsupported,
}

/// Pure dispatch helper: decide how to warm up the given active model at
/// app launch. No I/O; safe to unit test. Actual warmup is performed by
/// `VoiceWaveController::prewarm_active_model`.
pub fn warmup_plan_for_model(model_id: &str) -> WarmupPlan {
    if is_faster_whisper_model(model_id) {
        return WarmupPlan::FasterWhisper;
    }
    if is_whisper_cpp_model(model_id) {
        return WarmupPlan::WhisperCpp;
    }
    WarmupPlan::Unsupported
}

pub fn faster_whisper_runtime_model_id(model_id: &str) -> Option<&'static str> {
    match model_id {
        "fw-small.en" => Some("small.en"),
        "fw-large-v3" => Some("large-v3"),
        // faster-whisper >= 1.1 resolves this alias to the CT2 conversion of
        // OpenAI's pruned large-v3-turbo (near large-v3 accuracy, ~4x faster
        // decode, ~1.6 GB). Decode latency measured on par with small.en on
        // CUDA (301 ms vs 302 ms for 12 s of audio, beam 5).
        "fw-large-v3-turbo" => Some("large-v3-turbo"),
        _ => None,
    }
}

pub fn note_audio_pipeline_decode_success(version: &str) {
    match version {
        "v2" => audio_pipeline::note_decode_success(audio_pipeline::AudioPipelineVersion::V2),
        "v1" => audio_pipeline::note_decode_success(audio_pipeline::AudioPipelineVersion::V1),
        _ => {}
    }
}

pub fn note_audio_pipeline_decode_hard_failure() {
    audio_pipeline::note_decode_hard_failure(audio_pipeline::AudioPipelineVersion::V2);
}

#[derive(Debug, Clone)]
pub struct FasterWhisperPrefetchResult {
    pub model_init_ms: u64,
    pub runtime_cache_hit: bool,
    pub cache_hint_path: String,
}

pub async fn prefetch_faster_whisper_model(
    model_id: &str,
) -> Result<FasterWhisperPrefetchResult, InferenceError> {
    let runtime_model =
        faster_whisper_runtime_model_id(model_id).ok_or_else(|| InferenceError::DecodeFailed {
            model_id: model_id.to_string(),
            reason: "unsupported faster-whisper model id".to_string(),
        })?;
    let prefetch = faster_whisper::prefetch_model(runtime_model).await?;
    let cache_hint = faster_whisper::cache_hint_for_model(runtime_model);
    Ok(FasterWhisperPrefetchResult {
        model_init_ms: prefetch.model_init_ms,
        runtime_cache_hit: prefetch.runtime_cache_hit,
        cache_hint_path: cache_hint.to_string_lossy().to_string(),
    })
}

/// Warm the GPU for an imminent decode (dictation just started). No-op if
/// the GPU did real work in the last few seconds.
pub async fn warmup_faster_whisper_model(model_id: &str) -> Result<(), InferenceError> {
    let runtime_model =
        faster_whisper_runtime_model_id(model_id).ok_or_else(|| InferenceError::DecodeFailed {
            model_id: model_id.to_string(),
            reason: "unsupported faster-whisper model id".to_string(),
        })?;
    faster_whisper::warmup_model(runtime_model).await
}

async fn transcribe_with_whisper(
    samples: Vec<f32>,
    model_id: String,
    model_path: PathBuf,
    decode_mode: DecodeMode,
    cancel_token: CancellationToken,
) -> Result<WhisperDecodeOutput, InferenceError> {
    if cancel_token.is_cancelled() {
        return Err(InferenceError::Cancelled);
    }

    if cpu_runtime_pool_enabled() {
        match executor::decode_with_runtime_pool(
            samples.clone(),
            model_id.clone(),
            model_path.clone(),
            decode_mode,
            cancel_token.clone(),
        )
        .await
        {
            Ok(result) => return Ok(result),
            Err(err) => {
                eprintln!(
                    "voicewave: runtime pool decode failed for model '{}', falling back to cold path: {}",
                    model_id, err
                );
            }
        }
    }

    tokio::task::spawn_blocking(move || {
        cold_decode_whisper_blocking(&samples, &model_id, &model_path, decode_mode, &cancel_token)
    })
    .await
    .map_err(|err| InferenceError::RuntimeJoin(err.to_string()))?
}

async fn transcribe_with_faster_whisper(
    samples: Vec<f32>,
    model_id: String,
    decode_mode: DecodeMode,
    terminology_hint: Option<String>,
    cancel_token: CancellationToken,
) -> Result<WhisperDecodeOutput, InferenceError> {
    if cancel_token.is_cancelled() {
        return Err(InferenceError::Cancelled);
    }

    if samples.is_empty() {
        return Err(InferenceError::DecodeFailed {
            model_id,
            reason: "no usable audio samples captured".to_string(),
        });
    }
    let condition_started = Instant::now();
    let baseline_processed = audio_pipeline::process_for_faster_whisper_primary(&samples);
    let audio_condition_ms = condition_started.elapsed().as_millis() as u64;
    let conditioned_samples = baseline_processed.samples;
    if conditioned_samples.is_empty() {
        return Err(InferenceError::DecodeFailed {
            model_id,
            reason: "no usable audio samples after conditioning".to_string(),
        });
    }

    let runtime_model =
        faster_whisper_runtime_model_id(&model_id).ok_or_else(|| InferenceError::DecodeFailed {
            model_id: model_id.clone(),
            reason: "unsupported faster-whisper model id".to_string(),
        })?;
    let audio_duration_ms = ((conditioned_samples.len() as f64 / 16_000.0) * 1000.0).round() as u64;
    let baseline_run = run_faster_whisper_decode_with_conditioned(
        &conditioned_samples,
        runtime_model,
        &model_id,
        decode_mode,
        terminology_hint.as_deref(),
        audio_duration_ms,
        &cancel_token,
    )
    .await?;

    debug_fw_gpu_log(
        "GPU_H2",
        "src-tauri/src/inference/mod.rs:596",
        "fw decode complete",
        &[
            (
                "backendRequested",
                baseline_run.final_decode.backend_requested.clone(),
            ),
            (
                "backendUsed",
                baseline_run.final_decode.backend_used.clone(),
            ),
            (
                "backendFallback",
                baseline_run.final_decode.backend_fallback.to_string(),
            ),
            ("modelInitMs", baseline_run.total_model_init_ms.to_string()),
            (
                "decodeComputeMs",
                baseline_run.total_decode_compute_ms.to_string(),
            ),
            ("audioMs", audio_duration_ms.to_string()),
        ],
    );

    let mut shadow_candidate_version = None;
    let mut shadow_quality_delta = None;
    let mut shadow_candidate_avg_logprob = None;
    let mut shadow_candidate_no_speech_prob = None;
    let mut shadow_candidate_retry_used = None;
    let mut shadow_candidate_low_coherence = None;
    let mut shadow_candidate_decode_compute_ms = None;
    let mut shadow_candidate_won = None;
    if let Some(candidate_processed) =
        audio_pipeline::maybe_shadow_for_active(&samples, baseline_processed.version)
    {
        if !candidate_processed.samples.is_empty() && !cancel_token.is_cancelled() {
            if let Ok(candidate_run) = run_faster_whisper_decode_with_conditioned(
                &candidate_processed.samples,
                runtime_model,
                &model_id,
                decode_mode,
                terminology_hint.as_deref(),
                audio_duration_ms,
                &cancel_token,
            )
            .await
            {
                let baseline_quality = fw_decode_quality_score(
                    baseline_run.final_decode.avg_logprob,
                    baseline_run.final_decode.no_speech_prob,
                    baseline_run.low_coherence,
                    baseline_run.retry_used,
                );
                let candidate_quality = fw_decode_quality_score(
                    candidate_run.final_decode.avg_logprob,
                    candidate_run.final_decode.no_speech_prob,
                    candidate_run.low_coherence,
                    candidate_run.retry_used,
                );
                let quality_delta = candidate_quality - baseline_quality;
                let latency_budget = (baseline_run.total_decode_compute_ms as f32) * 1.08;
                let latency_ok = (candidate_run.total_decode_compute_ms as f32) <= latency_budget;
                let no_speech_ok = candidate_run.final_decode.no_speech_prob
                    <= baseline_run.final_decode.no_speech_prob + 0.03;
                shadow_candidate_version = Some(candidate_processed.version.as_str().to_string());
                shadow_quality_delta = Some(quality_delta);
                shadow_candidate_avg_logprob = Some(candidate_run.final_decode.avg_logprob);
                shadow_candidate_no_speech_prob = Some(candidate_run.final_decode.no_speech_prob);
                shadow_candidate_retry_used = Some(candidate_run.retry_used);
                shadow_candidate_low_coherence = Some(candidate_run.low_coherence);
                shadow_candidate_decode_compute_ms = Some(candidate_run.total_decode_compute_ms);
                shadow_candidate_won = Some(quality_delta >= 0.02 && latency_ok && no_speech_ok);
            }
        }
    }

    Ok(WhisperDecodeOutput {
        text: baseline_run.final_decode.text,
        telemetry: DecodeTelemetry {
            model_init_ms: baseline_run.total_model_init_ms,
            audio_condition_ms,
            decode_compute_ms: baseline_run.total_decode_compute_ms,
            audio_pipeline_version: baseline_processed.version.as_str().to_string(),
            runtime_cache_hit: baseline_run.runtime_cache_hit,
            backend_requested: if baseline_run
                .final_decode
                .backend_requested
                .trim()
                .is_empty()
            {
                "cpu".to_string()
            } else {
                baseline_run.final_decode.backend_requested
            },
            backend_used: if baseline_run.final_decode.backend_used.trim().is_empty() {
                "cpu".to_string()
            } else {
                baseline_run.final_decode.backend_used
            },
            backend_fallback: baseline_run.final_decode.backend_fallback,
            fw_segment_count: Some(baseline_run.final_decode.segment_count),
            fw_avg_logprob: Some(baseline_run.final_decode.avg_logprob),
            fw_no_speech_prob: Some(baseline_run.final_decode.no_speech_prob),
            fw_compression_ratio: Some(baseline_run.final_decode.compression_ratio),
            fw_low_coherence: baseline_run.low_coherence,
            fw_retry_used: baseline_run.retry_used,
            fw_literal_retry_used: baseline_run.literal_retry_used,
            fw_shadow_candidate_version: shadow_candidate_version,
            fw_shadow_quality_delta: shadow_quality_delta,
            fw_shadow_candidate_avg_logprob: shadow_candidate_avg_logprob,
            fw_shadow_candidate_no_speech_prob: shadow_candidate_no_speech_prob,
            fw_shadow_candidate_retry_used: shadow_candidate_retry_used,
            fw_shadow_candidate_low_coherence: shadow_candidate_low_coherence,
            fw_shadow_candidate_decode_compute_ms: shadow_candidate_decode_compute_ms,
            fw_shadow_candidate_won: shadow_candidate_won,
            audio_pipeline_fallback_engaged: baseline_processed.version
                == audio_pipeline::AudioPipelineVersion::V1,
            audio_pipeline_fallback_remaining: audio_pipeline::fallback_remaining_utterances(),
        },
    })
}

struct FasterWhisperRunResult {
    final_decode: faster_whisper::FasterWhisperDecodeOutput,
    total_model_init_ms: u64,
    total_decode_compute_ms: u64,
    runtime_cache_hit: bool,
    low_coherence: bool,
    retry_used: bool,
    literal_retry_used: bool,
}

async fn run_faster_whisper_decode_with_conditioned(
    conditioned_samples: &[f32],
    runtime_model: &str,
    frontend_model_id: &str,
    decode_mode: DecodeMode,
    terminology_hint: Option<&str>,
    audio_duration_ms: u64,
    cancel_token: &CancellationToken,
) -> Result<FasterWhisperRunResult, InferenceError> {
    let primary_mode = if should_use_fast_first_primary(decode_mode, audio_duration_ms) {
        DecodeMode::Fast
    } else {
        decode_mode
    };

    debug_fw_gpu_log(
        "GPU_H1",
        "src-tauri/src/inference/mod.rs:573",
        "fw primary decode start",
        &[
            ("modelId", runtime_model.to_string()),
            ("frontendModelId", frontend_model_id.to_string()),
            ("primaryMode", format!("{:?}", primary_mode)),
            ("requestedMode", format!("{:?}", decode_mode)),
            ("audioMs", audio_duration_ms.to_string()),
        ],
    );

    let primary = faster_whisper::transcribe_samples_with_overrides(
        conditioned_samples,
        runtime_model,
        primary_mode,
        fw_primary_overrides(primary_mode, terminology_hint, audio_duration_ms),
        cancel_token,
    )
    .await?;
    if cancel_token.is_cancelled() {
        return Err(InferenceError::Cancelled);
    }

    let mut final_decode = primary.clone();
    let mut total_model_init_ms = primary.model_init_ms;
    let mut total_decode_compute_ms = primary.decode_compute_ms;
    let mut runtime_cache_hit = primary.runtime_cache_hit;
    let low_coherence = fw_decode_is_low_coherence(&primary, audio_duration_ms);
    let substitution_suspected = fw_decode_looks_common_substitution(&primary, audio_duration_ms);
    let needs_retry = fw_decode_needs_retry(&primary, audio_duration_ms)
        || (low_coherence && (primary.avg_logprob < -0.82 || substitution_suspected));
    let mut retry_used = false;
    let mut literal_retry_used = false;

    if needs_retry {
        let retry_mode = if primary_mode != decode_mode {
            decode_mode
        } else {
            stronger_decode_mode(decode_mode).unwrap_or(decode_mode)
        };
        let retry_overrides = if low_coherence {
            literal_retry_used = true;
            fw_literal_retry_overrides(retry_mode, terminology_hint)
        } else {
            FasterWhisperRequestOverrides::default()
        };
        let retry = faster_whisper::transcribe_samples_with_overrides(
            conditioned_samples,
            runtime_model,
            retry_mode,
            retry_overrides,
            cancel_token,
        )
        .await?;
        if cancel_token.is_cancelled() {
            return Err(InferenceError::Cancelled);
        }
        retry_used = true;
        total_model_init_ms = total_model_init_ms.saturating_add(retry.model_init_ms);
        total_decode_compute_ms = total_decode_compute_ms.saturating_add(retry.decode_compute_ms);
        runtime_cache_hit &= retry.runtime_cache_hit;
        if fw_retry_beats_primary(&primary, &retry, audio_duration_ms) {
            final_decode = retry;
        }
    }

    Ok(FasterWhisperRunResult {
        final_decode,
        total_model_init_ms,
        total_decode_compute_ms,
        runtime_cache_hit,
        low_coherence,
        retry_used,
        literal_retry_used,
    })
}

fn fw_decode_quality_score(
    avg_logprob: f32,
    no_speech_prob: f32,
    low_coherence: bool,
    retry_used: bool,
) -> f32 {
    let logprob_score = ((avg_logprob + 1.5) / 1.5).clamp(0.0, 1.0);
    let no_speech_score = (1.0 - no_speech_prob).clamp(0.0, 1.0);
    let coherence_score = if low_coherence { 0.0 } else { 1.0 };
    let retry_score = if retry_used { 0.0 } else { 1.0 };
    (0.35 * logprob_score)
        + (0.25 * no_speech_score)
        + (0.20 * coherence_score)
        + (0.20 * retry_score)
}

fn stronger_decode_mode(mode: DecodeMode) -> Option<DecodeMode> {
    match mode {
        DecodeMode::Fast => Some(DecodeMode::Balanced),
        DecodeMode::Balanced => Some(DecodeMode::Quality),
        DecodeMode::Quality => None,
    }
}

fn fast_first_enabled() -> bool {
    // Opt-in only. Fast-first ran the primary decode at beam=1 greedy for every
    // short utterance and relied on confidence heuristics to catch bad output,
    // but greedy decodes are often confidently wrong (avg_logprob passes the
    // acceptance gate with incorrect words), so most short dictations shipped
    // the weak decode. Verified 2026-06-12: beam=1 vs beam=5 produce different
    // transcripts on the same capture, and beam=5 costs ~0.3 s on GPU.
    std::env::var("VOICEWAVE_FW_FAST_FIRST_ENABLED")
        .map(|value| {
            let normalized = value.trim().to_ascii_lowercase();
            normalized == "1" || normalized == "true" || normalized == "on"
        })
        .unwrap_or(false)
}

fn should_use_fast_first_primary(decode_mode: DecodeMode, audio_duration_ms: u64) -> bool {
    fast_first_enabled()
        && decode_mode == DecodeMode::Balanced
        && audio_duration_ms <= fast_first_primary_max_audio_ms()
}

fn fast_first_primary_max_audio_ms() -> u64 {
    std::env::var("VOICEWAVE_FW_FAST_FIRST_MAX_AUDIO_MS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|value| value.clamp(1_200, 10_000))
        .unwrap_or(3_200)
}

fn fw_decode_needs_retry(
    decode: &faster_whisper::FasterWhisperDecodeOutput,
    audio_duration_ms: u64,
) -> bool {
    let words = word_count(&decode.text);
    if words == 0 {
        return true;
    }
    if fw_accept_short_high_confidence(decode, audio_duration_ms) {
        return false;
    }
    if decode.no_speech_prob > 0.76 && words <= 8 {
        return true;
    }
    if decode.avg_logprob < -1.2 {
        return true;
    }
    if decode.compression_ratio > 2.35 {
        return true;
    }
    if fw_decode_looks_common_substitution(decode, audio_duration_ms) {
        return true;
    }
    if audio_duration_ms >= 2_600 && words <= 2 && decode.avg_logprob < -0.72 {
        return true;
    }
    audio_duration_ms >= 5_000 && words <= 6
}

fn fw_accept_short_high_confidence(
    decode: &faster_whisper::FasterWhisperDecodeOutput,
    audio_duration_ms: u64,
) -> bool {
    let words = word_count(&decode.text);
    audio_duration_ms <= 3_200
        && words >= 1
        && decode.avg_logprob > -0.84
        && decode.no_speech_prob < 0.62
        && decode.compression_ratio < 2.4
        && !fw_decode_looks_common_substitution(decode, audio_duration_ms)
}

fn fw_decode_is_low_coherence(
    decode: &faster_whisper::FasterWhisperDecodeOutput,
    audio_duration_ms: u64,
) -> bool {
    let words = tokenized_words(&decode.text);
    if words.is_empty() {
        return true;
    }
    let diversity = lexical_diversity(&words);
    let repeated_bigrams = repeated_bigram_ratio(&words);
    let single_char_ratio = single_char_token_ratio(&words);

    if words.len() >= 6
        && diversity > 0.72
        && repeated_bigrams < 0.16
        && decode.avg_logprob > -0.8
        && decode.no_speech_prob < 0.35
    {
        return false;
    }

    if decode.compression_ratio >= 2.7 || repeated_bigrams >= 0.4 {
        return true;
    }
    if audio_duration_ms >= 2_400 && words.len() <= 3 && decode.avg_logprob < -0.9 {
        return true;
    }
    decode.avg_logprob < -1.1
        && decode.no_speech_prob > 0.5
        && (diversity < 0.48 || single_char_ratio >= 0.45)
}

fn fw_retry_beats_primary(
    primary: &faster_whisper::FasterWhisperDecodeOutput,
    retry: &faster_whisper::FasterWhisperDecodeOutput,
    audio_duration_ms: u64,
) -> bool {
    let primary_words = word_count(&primary.text);
    let retry_words = word_count(&retry.text);
    if primary_words == 0 {
        return retry_words > 0;
    }
    if retry_words == 0 {
        return false;
    }

    let primary_score = fw_confidence_score(primary);
    let retry_score = fw_confidence_score(retry);
    if retry_score > primary_score + 0.04 {
        return true;
    }

    if audio_duration_ms >= 2_200
        && retry_words >= primary_words.saturating_add(3)
        && retry.no_speech_prob <= primary.no_speech_prob + 0.08
    {
        return true;
    }

    false
}

fn fw_confidence_score(decode: &faster_whisper::FasterWhisperDecodeOutput) -> f32 {
    let words = word_count(&decode.text).min(24) as f32;
    let compression_penalty = (decode.compression_ratio - 2.1).max(0.0) * 0.35;
    decode.avg_logprob - (decode.no_speech_prob * 1.25) + (words * 0.025) - compression_penalty
}

fn word_count(text: &str) -> usize {
    text.split_whitespace()
        .filter(|token| !token.is_empty())
        .count()
}

fn tokenized_words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|token| {
            token
                .chars()
                .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '\'')
                .collect::<String>()
                .to_ascii_lowercase()
        })
        .filter(|token| !token.is_empty())
        .collect()
}

fn lexical_diversity(tokens: &[String]) -> f32 {
    if tokens.is_empty() {
        return 0.0;
    }
    let unique = tokens.iter().collect::<HashSet<_>>().len() as f32;
    unique / tokens.len() as f32
}

fn repeated_bigram_ratio(tokens: &[String]) -> f32 {
    if tokens.len() < 4 {
        return 0.0;
    }
    let mut seen = HashSet::new();
    let mut repeated = 0usize;
    let mut total = 0usize;
    for window in tokens.windows(2) {
        let key = format!("{} {}", window[0], window[1]);
        total += 1;
        if !seen.insert(key) {
            repeated += 1;
        }
    }
    if total == 0 {
        0.0
    } else {
        repeated as f32 / total as f32
    }
}

fn single_char_token_ratio(tokens: &[String]) -> f32 {
    if tokens.is_empty() {
        return 0.0;
    }
    let single = tokens.iter().filter(|token| token.len() == 1).count();
    single as f32 / tokens.len() as f32
}

fn fw_decode_looks_common_substitution(
    decode: &faster_whisper::FasterWhisperDecodeOutput,
    audio_duration_ms: u64,
) -> bool {
    let words = tokenized_words(&decode.text);
    if words.is_empty() || words.len() > 4 || audio_duration_ms < 700 {
        return false;
    }
    let common_words: HashSet<&'static str> = [
        "a", "an", "and", "are", "as", "at", "be", "can", "do", "for", "from", "go", "hard",
        "have", "in", "is", "it", "of", "on", "or", "our", "so", "that", "the", "this", "to",
        "was", "we", "with", "word", "work", "you", "your",
    ]
    .into_iter()
    .collect();
    let common = words
        .iter()
        .filter(|token| common_words.contains(token.as_str()))
        .count();
    let common_ratio = common as f32 / words.len() as f32;
    common_ratio >= 0.8 && decode.avg_logprob < -0.55
}

fn fw_primary_overrides(
    decode_mode: DecodeMode,
    terminology_hint: Option<&str>,
    audio_duration_ms: u64,
) -> FasterWhisperRequestOverrides {
    let without_timestamps = fw_without_timestamps_enabled() && decode_mode != DecodeMode::Quality;
    if decode_mode == DecodeMode::Fast && audio_duration_ms <= 3_200 {
        // For short utterances, skip prompt tokens on the fast-primary pass.
        return FasterWhisperRequestOverrides {
            without_timestamps: Some(without_timestamps),
            ..FasterWhisperRequestOverrides::default()
        };
    }
    let mut prompt =
        String::from("Transcribe verbatim. Preserve technical terms and uncommon words exactly.");
    if let Some(hint) = terminology_hint {
        if !hint.trim().is_empty() {
            prompt.push_str(" Technical terms: ");
            prompt.push_str(hint.trim());
        }
    }
    let prompt = if prompt.chars().count() > 360 {
        prompt.chars().take(360).collect::<String>()
    } else {
        prompt
    };
    match decode_mode {
        DecodeMode::Fast => FasterWhisperRequestOverrides {
            initial_prompt: Some(prompt),
            without_timestamps: Some(without_timestamps),
            ..FasterWhisperRequestOverrides::default()
        },
        DecodeMode::Balanced | DecodeMode::Quality => FasterWhisperRequestOverrides {
            initial_prompt: Some(prompt),
            temperature: Some(0.0),
            without_timestamps: Some(without_timestamps),
            ..FasterWhisperRequestOverrides::default()
        },
    }
}

fn fw_literal_retry_overrides(
    retry_mode: DecodeMode,
    terminology_hint: Option<&str>,
) -> FasterWhisperRequestOverrides {
    let (beam_size, best_of) = match retry_mode {
        DecodeMode::Fast => (2, 1),
        DecodeMode::Balanced => (4, 2),
        DecodeMode::Quality => (5, 3),
    };
    let mut prompt = String::from(
        "Transcribe exactly what is spoken. Keep uncommon words, names, and spelled tokens. Do not paraphrase.",
    );
    if let Some(hint) = terminology_hint {
        if !hint.trim().is_empty() {
            prompt.push_str(" Prefer these technical terms when acoustically plausible: ");
            prompt.push_str(hint.trim());
        }
    }
    if prompt.chars().count() > 420 {
        prompt = prompt.chars().take(420).collect();
    }
    let without_timestamps = fw_without_timestamps_enabled() && retry_mode != DecodeMode::Quality;
    FasterWhisperRequestOverrides {
        beam_size: Some(beam_size),
        best_of: Some(best_of),
        vad_filter: Some(false),
        condition_on_previous_text: Some(false),
        without_timestamps: Some(without_timestamps),
        initial_prompt: Some(prompt),
        temperature: Some(0.0),
        no_speech_threshold: Some(0.5),
        log_prob_threshold: Some(-1.3),
        compression_ratio_threshold: Some(2.1),
    }
}

fn fw_without_timestamps_enabled() -> bool {
    std::env::var("VOICEWAVE_FW_WITHOUT_TIMESTAMPS")
        .map(|value| {
            let normalized = value.trim().to_ascii_lowercase();
            !(normalized == "0" || normalized == "false" || normalized == "off")
        })
        .unwrap_or(true)
}

#[derive(Debug, Clone)]
pub(crate) struct WhisperDecodeOutput {
    pub text: String,
    pub telemetry: DecodeTelemetry,
}

pub(crate) fn cold_decode_whisper_blocking(
    samples: &[f32],
    model_id: &str,
    model_path: &Path,
    decode_mode: DecodeMode,
    cancel_token: &CancellationToken,
) -> Result<WhisperDecodeOutput, InferenceError> {
    validate_model_artifact(model_path)?;
    let model_path_str = model_path.to_string_lossy().to_string();
    let context_init = initialize_context_with_backend(model_id, &model_path_str)?;
    decode_with_context(
        &context_init.context,
        samples,
        model_id,
        decode_mode,
        cancel_token,
        context_init.model_init_ms,
        false,
        context_init.backend_requested,
        context_init.backend_used,
        context_init.backend_fallback,
    )
}

pub(crate) fn decode_with_context(
    ctx: &WhisperContext,
    samples: &[f32],
    model_id: &str,
    decode_mode: DecodeMode,
    cancel_token: &CancellationToken,
    model_init_ms: u64,
    runtime_cache_hit: bool,
    backend_requested: RuntimeBackend,
    backend_used: RuntimeBackend,
    backend_fallback: bool,
) -> Result<WhisperDecodeOutput, InferenceError> {
    let condition_started = Instant::now();
    let processed_audio = audio_pipeline::process_for_active(samples);
    let conditioned_samples = processed_audio.samples;
    let audio_condition_ms = condition_started.elapsed().as_millis() as u64;
    if conditioned_samples.is_empty() {
        return Err(InferenceError::DecodeFailed {
            model_id: model_id.to_string(),
            reason: "no usable audio samples after conditioning".to_string(),
        });
    }

    let profile = decode_profile_for_mode(model_id, decode_mode);
    let sampling_strategy = match profile.strategy {
        DecodeStrategy::Greedy { best_of } => SamplingStrategy::Greedy { best_of },
        DecodeStrategy::BeamSearch {
            beam_size,
            patience,
        } => SamplingStrategy::BeamSearch {
            beam_size,
            patience,
        },
    };
    let mut params = FullParams::new(sampling_strategy);
    params.set_language(Some("en"));
    params.set_translate(false);
    params.set_no_context(profile.no_context);
    params.set_no_timestamps(true);
    params.set_single_segment(false);
    params.set_max_len(0);
    params.set_max_tokens(0);
    params.set_suppress_blank(true);
    params.set_suppress_nst(true);
    params.set_temperature(0.0);
    params.set_temperature_inc(0.0);
    params.set_no_speech_thold(profile.no_speech_thold);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_n_threads(
        std::thread::available_parallelism()
            .map(|value| value.get())
            .unwrap_or(2)
            .clamp(1, profile.thread_cap) as i32,
    );
    let token_for_abort = cancel_token.clone();
    let abort_callback: Box<dyn FnMut() -> bool> = Box::new(move || token_for_abort.is_cancelled());
    params.set_abort_callback_safe::<_, Box<dyn FnMut() -> bool>>(Some(abort_callback));

    let mut state = ctx
        .create_state()
        .map_err(|err| InferenceError::StateInitFailed(err.to_string()))?;
    let decode_started = Instant::now();
    state.full(params, &conditioned_samples).map_err(|err| {
        if cancel_token.is_cancelled() {
            InferenceError::Cancelled
        } else {
            InferenceError::DecodeFailed {
                model_id: model_id.to_string(),
                reason: err.to_string(),
            }
        }
    })?;

    if cancel_token.is_cancelled() {
        return Err(InferenceError::Cancelled);
    }

    let mut transcript = String::new();
    for segment in state.as_iter() {
        let text = segment.to_string();
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !transcript.is_empty() {
            transcript.push(' ');
        }
        transcript.push_str(trimmed);
    }

    if transcript.trim().is_empty() {
        return Err(InferenceError::DecodeFailed {
            model_id: model_id.to_string(),
            reason: "whisper returned empty transcript".to_string(),
        });
    }

    Ok(WhisperDecodeOutput {
        text: transcript,
        telemetry: DecodeTelemetry {
            model_init_ms,
            audio_condition_ms,
            decode_compute_ms: decode_started.elapsed().as_millis() as u64,
            audio_pipeline_version: processed_audio.version.as_str().to_string(),
            runtime_cache_hit,
            backend_requested: backend_requested.as_str().to_string(),
            backend_used: backend_used.as_str().to_string(),
            backend_fallback,
            fw_segment_count: None,
            fw_avg_logprob: None,
            fw_no_speech_prob: None,
            fw_compression_ratio: None,
            fw_low_coherence: false,
            fw_retry_used: false,
            fw_literal_retry_used: false,
            fw_shadow_candidate_version: None,
            fw_shadow_quality_delta: None,
            fw_shadow_candidate_avg_logprob: None,
            fw_shadow_candidate_no_speech_prob: None,
            fw_shadow_candidate_retry_used: None,
            fw_shadow_candidate_low_coherence: None,
            fw_shadow_candidate_decode_compute_ms: None,
            fw_shadow_candidate_won: None,
            audio_pipeline_fallback_engaged: false,
            audio_pipeline_fallback_remaining: 0,
        },
    })
}

pub(crate) struct ContextInitResult {
    pub context: WhisperContext,
    pub model_init_ms: u64,
    pub backend_requested: RuntimeBackend,
    pub backend_used: RuntimeBackend,
    pub backend_fallback: bool,
}

pub(crate) fn initialize_context_with_backend(
    model_id: &str,
    model_path: &str,
) -> Result<ContextInitResult, InferenceError> {
    let requested_backend = preferred_backend_for_model(model_id);
    let init_started = Instant::now();
    let requested_params = context_params_for_backend(requested_backend);

    match WhisperContext::new_with_params(model_path, requested_params) {
        Ok(context) => {
            let model_init_ms = init_started.elapsed().as_millis() as u64;
            Ok(ContextInitResult {
                context,
                model_init_ms,
                backend_requested: requested_backend,
                backend_used: requested_backend,
                backend_fallback: false,
            })
        }
        Err(requested_err) => {
            if requested_backend == RuntimeBackend::Cpu {
                return Err(InferenceError::ModelLoadFailed {
                    model_id: model_id.to_string(),
                    path: model_path.to_string(),
                    reason: requested_err.to_string(),
                });
            }
            let gpu_locked = note_gpu_runtime_failure();
            let lock_note = if gpu_locked {
                " gpu backend locked to CPU for this app session after repeated failures."
            } else {
                ""
            };

            let fallback_backend = RuntimeBackend::Cpu;
            let fallback_params = context_params_for_backend(fallback_backend);
            WhisperContext::new_with_params(model_path, fallback_params)
                .map(|context| ContextInitResult {
                    context,
                    model_init_ms: init_started.elapsed().as_millis() as u64,
                    backend_requested: requested_backend,
                    backend_used: fallback_backend,
                    backend_fallback: true,
                })
                .map_err(|fallback_err| InferenceError::ModelLoadFailed {
                    model_id: model_id.to_string(),
                    path: model_path.to_string(),
                    reason: format!(
                        "preferred backend '{}' failed: {}; cpu fallback failed: {}.{}",
                        requested_backend.as_str(),
                        requested_err,
                        fallback_err,
                        lock_note
                    ),
                })
        }
    }
}

pub(crate) fn validate_model_artifact(model_path: &Path) -> Result<(), InferenceError> {
    if !model_path.exists() {
        return Err(InferenceError::ModelFileMissing {
            path: model_path.to_string_lossy().to_string(),
        });
    }

    let is_supported = model_path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("gguf") || ext.eq_ignore_ascii_case("bin"));
    if !is_supported {
        return Err(InferenceError::ModelFormatUnsupported {
            path: model_path.to_string_lossy().to_string(),
        });
    }
    Ok(())
}

pub(crate) fn model_artifact_fingerprint(model_path: &Path) -> Result<String, InferenceError> {
    let metadata =
        std::fs::metadata(model_path).map_err(|err| InferenceError::ModelLoadFailed {
            model_id: "unknown".to_string(),
            path: model_path.to_string_lossy().to_string(),
            reason: format!("failed to read metadata: {err}"),
        })?;
    let modified_secs = metadata
        .modified()
        .ok()
        .and_then(|row| row.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|row| row.as_secs())
        .unwrap_or_default();
    Ok(format!("{}:{modified_secs}", metadata.len()))
}

pub(crate) fn decode_profile_version(model_id: &str, decode_mode: DecodeMode) -> String {
    let profile = decode_profile_for_mode(model_id, decode_mode);
    let strategy = match profile.strategy {
        DecodeStrategy::Greedy { best_of } => format!("g:{best_of}"),
        DecodeStrategy::BeamSearch {
            beam_size,
            patience,
        } => format!("b:{beam_size}:{patience:.2}"),
    };
    format!(
        "{strategy}:{}:{}:{}:{}",
        profile.partial_delay_ms, profile.no_speech_thold, profile.thread_cap, profile.no_context
    )
}

#[cfg(test)]
fn condition_audio_for_decode(samples: &[f32]) -> Vec<f32> {
    const FRAME_SIZE: usize = 320;
    const MIN_EDGE_RMS: f32 = 0.002;
    const MAX_EDGE_RMS: f32 = 0.03;
    const TARGET_RMS: f32 = 0.065;
    const MAX_PEAK: f32 = 0.95;
    const MIN_GAIN: f32 = 0.5;
    const MAX_GAIN: f32 = 4.0;
    const MIN_TRIMMED_RATIO: f32 = 0.35;

    if samples.is_empty() {
        return Vec::new();
    }

    let global_rms = rms(samples);
    let edge_threshold = (global_rms * 0.2).clamp(MIN_EDGE_RMS, MAX_EDGE_RMS);
    let trimmed = trim_low_energy_edges(samples, FRAME_SIZE, edge_threshold);
    if trimmed.is_empty() {
        return Vec::new();
    }
    let use_original_window = samples.len() >= FRAME_SIZE * 8
        && (trimmed.len() as f32 / samples.len() as f32) < MIN_TRIMMED_RATIO;
    let working = if use_original_window {
        samples.to_vec()
    } else {
        trimmed
    };

    let mean = working.iter().copied().sum::<f32>() / working.len() as f32;
    let mut centered = working
        .iter()
        .map(|sample| sample - mean)
        .collect::<Vec<f32>>();
    high_pass_filter_in_place(&mut centered, 90.0, 16_000.0);
    attenuate_low_noise_frames(&mut centered);

    let current_rms = rms(&centered);
    let peak = centered
        .iter()
        .fold(0.0_f32, |acc, sample| acc.max(sample.abs()));

    let rms_gain = if current_rms > 0.0 {
        TARGET_RMS / current_rms
    } else {
        1.0
    };
    let peak_gain = if peak > 0.0 { MAX_PEAK / peak } else { 1.0 };
    let gain = rms_gain.min(peak_gain).clamp(MIN_GAIN, MAX_GAIN);

    for sample in &mut centered {
        *sample = (*sample * gain).clamp(-1.0, 1.0);
    }

    centered
}

#[cfg(test)]
fn high_pass_filter_in_place(samples: &mut [f32], cutoff_hz: f32, sample_rate_hz: f32) {
    if samples.len() < 2 || cutoff_hz <= 0.0 || sample_rate_hz <= 0.0 {
        return;
    }
    let dt = 1.0 / sample_rate_hz;
    let rc = 1.0 / (2.0 * std::f32::consts::PI * cutoff_hz);
    let alpha = rc / (rc + dt);
    let mut prev_input = samples[0];
    let mut prev_output = samples[0];
    for sample in samples.iter_mut().skip(1) {
        let input = *sample;
        let output = alpha * (prev_output + input - prev_input);
        *sample = output;
        prev_input = input;
        prev_output = output;
    }
}

#[cfg(test)]
fn attenuate_low_noise_frames(samples: &mut [f32]) {
    if samples.is_empty() {
        return;
    }
    let mut abs_values = samples
        .iter()
        .map(|sample| sample.abs())
        .collect::<Vec<f32>>();
    abs_values.sort_by(|a, b| a.total_cmp(b));
    let q1_idx = ((abs_values.len() - 1) as f32 * 0.25).round() as usize;
    let noise_floor = abs_values[q1_idx];
    if noise_floor < 0.0012 {
        return;
    }
    let threshold = (noise_floor * 2.2).max(0.001);
    let attenuation = 0.75_f32;
    for sample in samples.iter_mut() {
        if sample.abs() < threshold {
            *sample *= attenuation;
        }
    }
}

#[cfg(test)]
fn trim_low_energy_edges(samples: &[f32], frame_size: usize, threshold: f32) -> Vec<f32> {
    if samples.is_empty() || frame_size == 0 {
        return Vec::new();
    }

    let mut first_voiced_frame = None::<usize>;
    let mut last_voiced_frame = None::<usize>;

    for (idx, frame) in samples.chunks(frame_size).enumerate() {
        if rms(frame) >= threshold {
            if first_voiced_frame.is_none() {
                first_voiced_frame = Some(idx);
            }
            last_voiced_frame = Some(idx);
        }
    }

    let Some(first) = first_voiced_frame else {
        return samples.to_vec();
    };
    let last = last_voiced_frame.unwrap_or(first);

    let start_frame = first.saturating_sub(1);
    let end_frame_exclusive = last + 2;
    let start = (start_frame * frame_size).min(samples.len());
    let end = (end_frame_exclusive * frame_size).min(samples.len());

    if start >= end {
        return samples.to_vec();
    }

    samples[start..end].to_vec()
}

#[cfg(test)]
fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let power_sum: f32 = samples.iter().map(|sample| sample * sample).sum();
    (power_sum / samples.len() as f32).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_whisper_cpp_model_identifies_wcpp_prefix() {
        assert!(is_whisper_cpp_model("wcpp-small.en"));
        assert!(is_whisper_cpp_model("wcpp-large-v3-turbo"));
        assert!(!is_whisper_cpp_model("fw-small.en"));
        assert!(!is_whisper_cpp_model("fw-large-v3"));
        assert!(!is_whisper_cpp_model(""));
    }

    #[test]
    fn warmup_plan_routes_faster_whisper_models_to_python_prefetch() {
        // fw-* models must be routed to the Python worker prefetch path so
        // the model is loaded into MODEL_CACHE at app startup.
        assert_eq!(warmup_plan_for_model("fw-small.en"), WarmupPlan::FasterWhisper);
        assert_eq!(warmup_plan_for_model("fw-large-v3"), WarmupPlan::FasterWhisper);
    }

    #[test]
    fn warmup_plan_routes_whisper_cpp_models_to_prewarm_runtime() {
        // wcpp-* models use the existing Rust prewarm_runtime path so the
        // WhisperContext is initialized before the first transcription.
        assert_eq!(warmup_plan_for_model("wcpp-small.en"), WarmupPlan::WhisperCpp);
        assert_eq!(
            warmup_plan_for_model("wcpp-large-v3-turbo"),
            WarmupPlan::WhisperCpp
        );
    }

    #[test]
    fn warmup_plan_skips_unknown_or_empty_model_ids() {
        // Unknown or unset model IDs should not crash startup. Return
        // Unsupported and let the caller skip the warmup call.
        assert_eq!(warmup_plan_for_model(""), WarmupPlan::Unsupported);
        assert_eq!(warmup_plan_for_model("some-future-backend"), WarmupPlan::Unsupported);
    }

    #[test]
    fn wcpp_models_use_beam_search_not_greedy_default() {
        // Greedy is the default fallback profile and produces noticeably worse
        // accuracy on Whisper. wcpp models must opt into BeamSearch like fw.
        let small = decode_profile_for_mode("wcpp-small.en", DecodeMode::Balanced);
        assert!(
            matches!(small.strategy, DecodeStrategy::BeamSearch { .. }),
            "wcpp-small.en should use BeamSearch, got {:?}",
            small.strategy
        );
        let large = decode_profile_for_mode("wcpp-large-v3-turbo", DecodeMode::Balanced);
        assert!(
            matches!(large.strategy, DecodeStrategy::BeamSearch { .. }),
            "wcpp-large-v3-turbo should use BeamSearch, got {:?}",
            large.strategy
        );
    }

    fn fw_decode(
        text: &str,
        avg_logprob: f32,
        no_speech_prob: f32,
        compression_ratio: f32,
    ) -> faster_whisper::FasterWhisperDecodeOutput {
        faster_whisper::FasterWhisperDecodeOutput {
            text: text.to_string(),
            model_init_ms: 0,
            decode_compute_ms: 0,
            runtime_cache_hit: true,
            segment_count: 1,
            avg_logprob,
            no_speech_prob,
            compression_ratio,
            backend_requested: "cpu".to_string(),
            backend_used: "cpu".to_string(),
            backend_fallback: false,
        }
    }

    fn profile_sweep(
        model_ids: &[&str],
        modes: &[DecodeMode],
    ) -> Vec<(String, DecodeMode, ModelDecodeProfile)> {
        let mut rows = Vec::new();
        for model_id in model_ids {
            for mode in modes {
                rows.push((
                    (*model_id).to_string(),
                    *mode,
                    decode_profile_for_mode(model_id, *mode),
                ));
            }
        }
        rows
    }

    #[tokio::test]
    async fn worker_emits_partials_and_final() {
        let worker =
            InferenceWorker::with_script("small.en", "phase one is ready").with_partial_delay_ms(1);
        let token = CancellationToken::new();
        let mut partial_count = 0usize;
        let output = worker
            .transcribe_segment(&vec![0.01; 16_000], &token, |_, is_final, _| {
                if !is_final {
                    partial_count += 1;
                }
            })
            .await
            .expect("scripted inference should succeed");

        assert_eq!(output.transcript.as_deref(), Some("phase one is ready"));
        assert!(partial_count >= 2);
    }

    #[tokio::test]
    async fn cancellation_interrupts_decode() {
        let worker = InferenceWorker::with_script("small.en", "this should not finish")
            .with_partial_delay_ms(1);
        let token = CancellationToken::new();
        token.cancel();
        let output = worker
            .transcribe_segment(&vec![0.01; 16_000], &token, |_, _, _| {})
            .await
            .expect("scripted worker should not fail on cancellation");
        assert!(output.transcript.is_none());
    }

    #[test]
    fn decode_profiles_scale_by_model() {
        assert!(
            decode_profile("tiny.en").partial_delay_ms
                < decode_profile("medium.en").partial_delay_ms
        );
    }

    #[test]
    fn rtf_calculation_is_nonzero_for_valid_audio() {
        let rtf = estimate_rtf(500, 16_000);
        assert!(rtf > 0.0);
    }

    #[test]
    fn runtime_constructor_tracks_model_path() {
        let worker = InferenceWorker::new_runtime("small.en", "C:\\models\\small.en.gguf");
        assert_eq!(
            worker
                .model_path()
                .map(|path| path.to_string_lossy().to_string()),
            Some("C:\\models\\small.en.gguf".to_string())
        );
    }

    #[test]
    fn conditioning_trims_edges_and_boosts_quiet_audio() {
        let mut samples = vec![0.0_f32; 320 * 6];
        samples.extend((0..320 * 12).map(|i| ((i as f32 * 0.03).sin()) * 0.008));
        samples.extend(vec![0.0_f32; 320 * 6]);

        let conditioned = condition_audio_for_decode(&samples);
        assert!(!conditioned.is_empty());
        assert!(conditioned.len() < samples.len());
        assert!(rms(&conditioned) > rms(&samples));
    }

    #[test]
    fn high_pass_filter_reduces_dc_offset_component() {
        let mut samples = vec![0.15_f32; 16_000];
        high_pass_filter_in_place(&mut samples, 90.0, 16_000.0);
        assert!(rms(&samples) < 0.02);
    }

    #[test]
    fn decode_profile_prefers_beam_for_larger_models() {
        assert!(matches!(
            decode_profile("small.en").strategy,
            DecodeStrategy::BeamSearch { .. }
        ));
        assert!(matches!(
            decode_profile("tiny.en").strategy,
            DecodeStrategy::Greedy { .. }
        ));
    }

    #[test]
    fn fast_mode_uses_greedy_strategy() {
        let profile = decode_profile_for_mode("small.en", DecodeMode::Fast);
        assert!(matches!(
            profile.strategy,
            DecodeStrategy::Greedy { best_of: 1 }
        ));
        assert!(profile.no_context);
    }

    #[test]
    fn profile_sweep_covers_tiny_and_small_for_balanced_and_fast_modes() {
        let rows = profile_sweep(
            &["tiny.en", "small.en"],
            &[DecodeMode::Balanced, DecodeMode::Fast],
        );
        assert_eq!(rows.len(), 4);

        let small_balanced = rows
            .iter()
            .find(|(model_id, mode, _)| model_id == "small.en" && *mode == DecodeMode::Balanced)
            .expect("small.en balanced profile should exist");
        let small_fast = rows
            .iter()
            .find(|(model_id, mode, _)| model_id == "small.en" && *mode == DecodeMode::Fast)
            .expect("small.en fast profile should exist");

        assert!(small_fast.2.partial_delay_ms <= small_balanced.2.partial_delay_ms);
        assert!(small_balanced.2.thread_cap >= 10);
    }

    #[test]
    fn fw_retry_detects_low_confidence_decode() {
        let weak = fw_decode("quick note", -1.35, 0.75, 2.5);
        assert!(fw_decode_needs_retry(&weak, 1_600));
    }

    #[test]
    fn fw_retry_skips_high_confidence_decode() {
        let strong = fw_decode("voicewave dictation looks stable now", -0.42, 0.18, 1.2);
        assert!(!fw_decode_needs_retry(&strong, 1_600));
    }

    #[test]
    fn fw_retry_prefers_better_confidence_output() {
        let primary = fw_decode("hey", -1.26, 0.74, 2.4);
        let retry = fw_decode("hey team let's ship this fix", -0.62, 0.26, 1.3);
        assert!(fw_retry_beats_primary(&primary, &retry, 2_500));
    }

    #[test]
    fn fw_detects_low_coherence_with_repetition_pressure() {
        let weak = fw_decode("this this this this this this", -0.96, 0.22, 2.6);
        assert!(fw_decode_is_low_coherence(&weak, 1_900));
    }

    #[test]
    fn fw_literal_retry_profile_is_prompted_and_context_free() {
        let profile =
            fw_literal_retry_overrides(DecodeMode::Balanced, Some("Kubernetes protobuf gRPC"));
        assert_eq!(profile.condition_on_previous_text, Some(false));
        assert_eq!(profile.temperature, Some(0.0));
        assert!(profile
            .initial_prompt
            .as_deref()
            .is_some_and(|text| text.contains("Transcribe exactly")));
        assert!(profile
            .initial_prompt
            .as_deref()
            .is_some_and(|text| text.contains("Kubernetes protobuf gRPC")));
    }

    #[test]
    fn fw_common_substitution_pattern_triggers_retry() {
        let weak = fw_decode("hard work", -0.72, 0.14, 1.6);
        assert!(fw_decode_looks_common_substitution(&weak, 900));
    }

    #[test]
    fn fw_primary_profile_includes_technical_hint_prompt() {
        let profile =
            fw_primary_overrides(DecodeMode::Balanced, Some("TypeScript ESLint Vite"), 4_000);
        assert!(profile
            .initial_prompt
            .as_deref()
            .is_some_and(|text| text.contains("TypeScript ESLint Vite")));
    }

    #[test]
    fn fw_fast_short_primary_skips_prompt_for_latency() {
        let profile = fw_primary_overrides(DecodeMode::Fast, Some("Rust"), 2_200);
        assert!(profile.initial_prompt.is_none());
    }

    #[test]
    fn stronger_mode_escalates_until_quality() {
        assert_eq!(
            stronger_decode_mode(DecodeMode::Fast),
            Some(DecodeMode::Balanced)
        );
        assert_eq!(
            stronger_decode_mode(DecodeMode::Balanced),
            Some(DecodeMode::Quality)
        );
        assert_eq!(stronger_decode_mode(DecodeMode::Quality), None);
    }

    #[test]
    fn fw_fast_first_is_disabled_by_default() {
        // Quality regression guard: the primary decode must never silently
        // drop to beam=1 greedy unless the user opts in via
        // VOICEWAVE_FW_FAST_FIRST_ENABLED.
        assert!(!should_use_fast_first_primary(DecodeMode::Balanced, 2_200));
        assert!(!should_use_fast_first_primary(DecodeMode::Balanced, 3_200));
        assert!(!should_use_fast_first_primary(DecodeMode::Balanced, 3_300));
    }

    #[test]
    fn fw_fast_first_never_applies_to_explicit_quality_modes() {
        assert!(!should_use_fast_first_primary(DecodeMode::Fast, 2_000));
        assert!(!should_use_fast_first_primary(DecodeMode::Quality, 2_000));
    }

    #[test]
    fn fw_quality_score_rewards_confident_non_retry_decode() {
        let strong = fw_decode_quality_score(-0.45, 0.12, false, false);
        let weak = fw_decode_quality_score(-1.25, 0.72, true, true);
        assert!(strong > weak);
    }
}
