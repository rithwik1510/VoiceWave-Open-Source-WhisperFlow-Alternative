use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU32, Ordering};

static V2_CONSECUTIVE_HARD_FAILURES: AtomicU32 = AtomicU32::new(0);
static V1_FALLBACK_UTTERANCES_REMAINING: AtomicU32 = AtomicU32::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioPipelineVersion {
    V1,
    V2,
}

impl AudioPipelineVersion {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::V1 => "v1",
            Self::V2 => "v2",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProcessedAudio {
    pub version: AudioPipelineVersion,
    pub samples: Vec<f32>,
}

pub fn process_for_active(samples: &[f32]) -> ProcessedAudio {
    let version = active_pipeline_version();
    ProcessedAudio {
        version,
        samples: process_for_version(samples, version),
    }
}

pub fn process_for_faster_whisper_primary(samples: &[f32]) -> ProcessedAudio {
    let version = select_primary_pipeline_for_faster_whisper();
    ProcessedAudio {
        version,
        samples: process_for_version(samples, version),
    }
}

pub fn note_decode_success(version: AudioPipelineVersion) {
    let _ = version;
    V2_CONSECUTIVE_HARD_FAILURES.store(0, Ordering::Relaxed);
}

pub fn note_decode_hard_failure(version: AudioPipelineVersion) {
    if version != AudioPipelineVersion::V2 {
        return;
    }
    let failures = V2_CONSECUTIVE_HARD_FAILURES
        .fetch_add(1, Ordering::Relaxed)
        .saturating_add(1);
    if allow_v1_fallback()
        && failures >= fallback_failure_threshold()
        && V1_FALLBACK_UTTERANCES_REMAINING.load(Ordering::Relaxed) == 0
    {
        V1_FALLBACK_UTTERANCES_REMAINING.store(fallback_utterance_budget(), Ordering::Relaxed);
        V2_CONSECUTIVE_HARD_FAILURES.store(0, Ordering::Relaxed);
    }
}

pub fn fallback_remaining_utterances() -> u32 {
    V1_FALLBACK_UTTERANCES_REMAINING.load(Ordering::Relaxed)
}

pub fn maybe_shadow_for_active(
    samples: &[f32],
    active: AudioPipelineVersion,
) -> Option<ProcessedAudio> {
    if active == AudioPipelineVersion::V1 {
        return None;
    }
    if force_v1_enabled() || !shadow_enabled() || !shadow_sample_hit(samples) {
        return None;
    }
    let candidate = match active {
        AudioPipelineVersion::V1 => AudioPipelineVersion::V2,
        AudioPipelineVersion::V2 => {
            // Keep legacy v1 off by default when v2 is active.
            if env_flag("VOICEWAVE_AUDIO_PIPELINE_ALLOW_V1_SHADOW", false) {
                AudioPipelineVersion::V1
            } else {
                return None;
            }
        }
    };
    Some(ProcessedAudio {
        version: candidate,
        samples: process_for_version(samples, candidate),
    })
}

pub fn process_for_version(samples: &[f32], version: AudioPipelineVersion) -> Vec<f32> {
    match version {
        AudioPipelineVersion::V1 => condition_audio_v1(samples),
        AudioPipelineVersion::V2 => condition_audio_v2(samples),
    }
}

fn active_pipeline_version() -> AudioPipelineVersion {
    // v1 should only be reachable through explicit emergency switches.
    if force_v1_enabled() || emergency_v1_enabled() {
        return AudioPipelineVersion::V1;
    }
    AudioPipelineVersion::V2
}

fn select_primary_pipeline_for_faster_whisper() -> AudioPipelineVersion {
    if force_v1_enabled() || emergency_v1_enabled() {
        return AudioPipelineVersion::V1;
    }
    if !allow_v1_fallback() {
        return AudioPipelineVersion::V2;
    }
    let remaining = V1_FALLBACK_UTTERANCES_REMAINING.load(Ordering::Relaxed);
    if remaining == 0 {
        return AudioPipelineVersion::V2;
    }
    V1_FALLBACK_UTTERANCES_REMAINING.fetch_sub(1, Ordering::Relaxed);
    AudioPipelineVersion::V1
}

fn force_v1_enabled() -> bool {
    env_flag("VOICEWAVE_AUDIO_PIPELINE_FORCE_V1", false)
}

fn emergency_v1_enabled() -> bool {
    env_flag("VOICEWAVE_AUDIO_PIPELINE_EMERGENCY_V1", false)
}

fn shadow_enabled() -> bool {
    env_flag("VOICEWAVE_AUDIO_PIPELINE_SHADOW_ENABLED", false)
}

fn shadow_sample_pct() -> u32 {
    std::env::var("VOICEWAVE_AUDIO_PIPELINE_SHADOW_SAMPLE_PCT")
        .ok()
        .and_then(|value| value.trim().parse::<u32>().ok())
        .map(|value| value.clamp(0, 100))
        .unwrap_or(5)
}

fn allow_v1_fallback() -> bool {
    env_flag("VOICEWAVE_AUDIO_PIPELINE_ALLOW_V1_FALLBACK", true)
}

fn fallback_failure_threshold() -> u32 {
    std::env::var("VOICEWAVE_AUDIO_PIPELINE_FALLBACK_FAILURES")
        .ok()
        .and_then(|value| value.trim().parse::<u32>().ok())
        .map(|value| value.clamp(2, 8))
        .unwrap_or(3)
}

fn fallback_utterance_budget() -> u32 {
    std::env::var("VOICEWAVE_AUDIO_PIPELINE_FALLBACK_UTTERANCES")
        .ok()
        .and_then(|value| value.trim().parse::<u32>().ok())
        .map(|value| value.clamp(1, 6))
        .unwrap_or(2)
}

fn shadow_sample_hit(samples: &[f32]) -> bool {
    let pct = shadow_sample_pct();
    if pct == 0 {
        return false;
    }
    if pct >= 100 {
        return true;
    }
    let mut hasher = DefaultHasher::new();
    samples.len().hash(&mut hasher);
    for sample in samples.iter().step_by(97).take(64) {
        sample.to_bits().hash(&mut hasher);
    }
    let bucket = (hasher.finish() % 100) as u32;
    bucket < pct
}

fn condition_audio_v1(samples: &[f32]) -> Vec<f32> {
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

    normalize_gain_in_place(&mut centered, TARGET_RMS, MAX_PEAK, MIN_GAIN, MAX_GAIN);
    centered
}

fn condition_audio_v2(samples: &[f32]) -> Vec<f32> {
    const FRAME_SIZE: usize = 320;
    // Lowered from 0.0018 to 0.0008 to match audio::adaptive_preserve_threshold
    // at the capture layer. The old floor was too aggressive for quiet
    // speakers: their soft tails (RMS 0.002-0.004) fell below the floor
    // and `trim_low_energy_edges` clipped the last 2-3 words even though
    // the capture layer had preserved them. Must stay >= typical ambient
    // mic noise (~0.0005) so silence isn't classified as voiced.
    const MIN_EDGE_RMS: f32 = 0.0008;
    const MAX_EDGE_RMS: f32 = 0.028;
    const TARGET_RMS: f32 = 0.07;
    const MAX_PEAK: f32 = 0.94;
    const MIN_GAIN: f32 = 0.45;
    const MAX_GAIN: f32 = 4.5;
    const MIN_TRIMMED_RATIO: f32 = 0.30;

    if samples.is_empty() {
        return Vec::new();
    }

    let global_rms = rms(samples);
    let floor = percentile_abs(samples, 0.2);
    let adaptive_edge = (floor * 2.6).clamp(MIN_EDGE_RMS, MAX_EDGE_RMS);
    let edge_threshold = if env_flag("VOICEWAVE_AP2_ENABLE_ADAPTIVE_TRIM", true) {
        adaptive_edge
    } else {
        (global_rms * 0.2).clamp(MIN_EDGE_RMS, MAX_EDGE_RMS)
    };
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
    let mut staged = working
        .iter()
        .map(|sample| sample - mean)
        .collect::<Vec<f32>>();
    // Steps 5-9 below are all OFF by default. Whisper expects the audio as
    // captured: varying loudness, full low-end, unboosted highs. Each step
    // stays behind an env flag so it can be re-enabled for experimentation,
    // but none of them run on the clean default path.
    if env_flag("VOICEWAVE_AP2_ENABLE_HIGHPASS", false) {
        high_pass_filter_in_place(&mut staged, 90.0, 16_000.0);
    }
    if env_flag("VOICEWAVE_AP2_ENABLE_PREEMPHASIS", false) {
        pre_emphasis_in_place(&mut staged, 0.95);
    }
    if env_flag("VOICEWAVE_AP2_ENABLE_HUM_NOTCH", false) {
        notch_filter_in_place(&mut staged, 50.0, 16_000.0, 15.0);
        notch_filter_in_place(&mut staged, 60.0, 16_000.0, 15.0);
    }
    if env_flag("VOICEWAVE_AP2_ENABLE_NOISE_ATTENUATION", false) {
        if env_flag("VOICEWAVE_AP2_ENABLE_DYNAMIC_NOISE_ATTENUATION", true) {
            attenuate_low_noise_frames_dynamic(&mut staged);
        } else {
            attenuate_low_noise_frames(&mut staged);
        }
    }
    if env_flag("VOICEWAVE_AP2_ENABLE_GAIN_NORMALIZATION", false) {
        normalize_gain_in_place(&mut staged, TARGET_RMS, MAX_PEAK, MIN_GAIN, MAX_GAIN);
    }
    if env_flag("VOICEWAVE_AP2_ENABLE_SOFT_LIMITER", false) {
        soft_limiter_in_place(&mut staged, 0.95);
    }
    staged
}

fn normalize_gain_in_place(
    samples: &mut [f32],
    target_rms: f32,
    max_peak: f32,
    min_gain: f32,
    max_gain: f32,
) {
    let current_rms = rms(samples);
    let peak = samples
        .iter()
        .fold(0.0_f32, |acc, sample| acc.max(sample.abs()));

    let rms_gain = if current_rms > 0.0 {
        target_rms / current_rms
    } else {
        1.0
    };
    let peak_gain = if peak > 0.0 { max_peak / peak } else { 1.0 };
    let gain = rms_gain.min(peak_gain).clamp(min_gain, max_gain);

    for sample in samples.iter_mut() {
        *sample = (*sample * gain).clamp(-1.0, 1.0);
    }
}

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

fn pre_emphasis_in_place(samples: &mut [f32], coeff: f32) {
    if samples.len() < 2 {
        return;
    }
    let mut prev = samples[0];
    for sample in samples.iter_mut().skip(1) {
        let current = *sample;
        *sample = current - (coeff * prev);
        prev = current;
    }
}

fn notch_filter_in_place(samples: &mut [f32], freq_hz: f32, sample_rate_hz: f32, q: f32) {
    if samples.len() < 3 || freq_hz <= 0.0 || sample_rate_hz <= 0.0 || q <= 0.0 {
        return;
    }
    let omega = 2.0 * std::f32::consts::PI * freq_hz / sample_rate_hz;
    let alpha = omega.sin() / (2.0 * q);
    let cos_omega = omega.cos();

    let b0 = 1.0;
    let b1 = -2.0 * cos_omega;
    let b2 = 1.0;
    let a0 = 1.0 + alpha;
    let a1 = -2.0 * cos_omega;
    let a2 = 1.0 - alpha;

    let nb0 = b0 / a0;
    let nb1 = b1 / a0;
    let nb2 = b2 / a0;
    let na1 = a1 / a0;
    let na2 = a2 / a0;

    let mut x1 = 0.0;
    let mut x2 = 0.0;
    let mut y1 = 0.0;
    let mut y2 = 0.0;
    for sample in samples.iter_mut() {
        let x0 = *sample;
        let y0 = (nb0 * x0) + (nb1 * x1) + (nb2 * x2) - (na1 * y1) - (na2 * y2);
        *sample = y0;
        x2 = x1;
        x1 = x0;
        y2 = y1;
        y1 = y0;
    }
}

fn soft_limiter_in_place(samples: &mut [f32], knee: f32) {
    if knee <= 0.0 || knee >= 1.0 {
        return;
    }
    for sample in samples.iter_mut() {
        let sign = sample.signum();
        let value = sample.abs();
        if value > knee {
            let excess = value - knee;
            let compressed = knee + (excess / (1.0 + excess));
            *sample = (compressed * sign).clamp(-1.0, 1.0);
        }
    }
}

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

fn attenuate_low_noise_frames_dynamic(samples: &mut [f32]) {
    if samples.is_empty() {
        return;
    }
    let noise_floor = percentile_abs(samples, 0.2).max(0.0009);
    let threshold = (noise_floor * 2.0).max(0.001);
    let attenuation = 0.72_f32;
    for sample in samples.iter_mut() {
        if sample.abs() < threshold {
            *sample *= attenuation;
        }
    }
}

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

    // Head padding: 4 frames (~80 ms) so leading plosives (p/t/k) and
    // aspirated onsets are not clipped.
    const HEAD_PAD_FRAMES: usize = 4;
    // Tail padding: 15 frames (~300 ms) so soft word endings that drop below
    // the RMS threshold ("...house", "...about", "...thing") survive, AND so
    // Whisper receives an unambiguous trailing-silence cue instead of an
    // audio cliff — which is what was causing the last 2-3 words to drop.
    const TAIL_PAD_FRAMES: usize = 15;

    let start_frame = first.saturating_sub(HEAD_PAD_FRAMES);
    let end_frame_exclusive = last.saturating_add(1).saturating_add(TAIL_PAD_FRAMES);
    let start = start_frame.saturating_mul(frame_size).min(samples.len());
    let end = end_frame_exclusive
        .saturating_mul(frame_size)
        .min(samples.len());
    if start >= end {
        return samples.to_vec();
    }
    samples[start..end].to_vec()
}

fn percentile_abs(samples: &[f32], quantile: f32) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut values = samples.iter().map(|v| v.abs()).collect::<Vec<f32>>();
    let idx = ((values.len() - 1) as f32 * quantile.clamp(0.0, 1.0)).round() as usize;
    // Linear-time selection: same value a full sort would put at `idx`, but
    // O(n) instead of O(n log n) — this runs on every dictation's full buffer.
    let (_, value, _) = values.select_nth_unstable_by(idx, |a, b| a.total_cmp(b));
    *value
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let power_sum: f32 = samples.iter().map(|sample| sample * sample).sum();
    (power_sum / samples.len() as f32).sqrt()
}

fn env_flag(key: &str, default_value: bool) -> bool {
    match std::env::var(key) {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => true,
            "0" | "false" | "no" | "off" => false,
            _ => default_value,
        },
        Err(_) => default_value,
    }
}

#[cfg(test)]
mod tests {
    use super::{process_for_version, trim_low_energy_edges, AudioPipelineVersion};

    fn fixture_samples() -> Vec<f32> {
        let mut out = vec![0.0_f32; 320 * 4];
        out.extend((0..(320 * 20)).map(|i| ((i as f32 * 0.03).sin()) * 0.028));
        out.extend(vec![0.0_f32; 320 * 4]);
        out
    }

    #[test]
    fn v1_conditioning_returns_non_empty_for_valid_audio() {
        let conditioned = process_for_version(&fixture_samples(), AudioPipelineVersion::V1);
        assert!(!conditioned.is_empty());
    }

    #[test]
    fn v2_conditioning_returns_non_empty_for_valid_audio() {
        let conditioned = process_for_version(&fixture_samples(), AudioPipelineVersion::V2);
        assert!(!conditioned.is_empty());
    }

    #[test]
    fn v2_conditioning_is_not_identical_to_v1() {
        let source = fixture_samples();
        let v1 = process_for_version(&source, AudioPipelineVersion::V1);
        let v2 = process_for_version(&source, AudioPipelineVersion::V2);
        assert_ne!(v1, v2);
    }

    fn rms(samples: &[f32]) -> f32 {
        let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
        (sum_sq / samples.len().max(1) as f32).sqrt()
    }

    #[test]
    fn trim_low_energy_edges_keeps_generous_tail_for_soft_word_endings() {
        // Soft word endings ("...house", "...about", "...thing") live in the
        // final 150-300 ms of an utterance, often below the RMS trim
        // threshold. We must keep ~300 ms of tail past the last voiced frame
        // so those consonants aren't clipped — and so Whisper sees a proper
        // trailing-silence cue instead of an audio cliff.
        let frame_size = 320_usize;
        let voiced_frames = 60; // ~1.2 s of loud speech-band tone
        let trailing_silence_frames = 30; // ~600 ms of room to keep
        let threshold = 0.02_f32;
        let voiced_amplitude = 0.3_f32;

        let mut samples = Vec::new();
        for _ in 0..voiced_frames {
            for i in 0..frame_size {
                samples.push(
                    voiced_amplitude
                        * (2.0 * std::f32::consts::PI * 1_000.0 * i as f32 / 16_000.0).sin(),
                );
            }
        }
        samples.extend(vec![0.0_f32; frame_size * trailing_silence_frames]);

        let trimmed = trim_low_energy_edges(&samples, frame_size, threshold);
        let trimmed_frames = trimmed.len() / frame_size;

        // We need at least 10 frames (~200 ms) of tail past the last voiced
        // frame. Current code keeps 1 frame (~20 ms) which clips soft endings.
        assert!(
            trimmed_frames >= voiced_frames + 10,
            "trim must keep at least 10 frames of tail past last voiced frame; \
             kept {trimmed_frames} frames total (voiced={voiced_frames}, \
             min expected total = {})",
            voiced_frames + 10
        );
    }

    #[test]
    fn v2_defaults_preserve_input_loudness_within_10_percent() {
        // Whisper wants the audio as captured — no forced gain normalization,
        // no pre-emphasis, no 90 Hz high-pass. A clean speech-band tone at
        // moderate loudness must pass through with its RMS essentially intact.
        //
        // If any of the legacy aggressive DSP steps (gain norm to 0.07 RMS,
        // pre-emphasis, high-pass, noise attenuation, soft limiter) are on
        // by default, this test fails because the output RMS is forcibly
        // rewritten or spectrally filtered.
        let duration_samples = 16_000 * 2; // 2 s at 16 kHz, well above trim window
        let input_amplitude = 0.3_f32;
        let input: Vec<f32> = (0..duration_samples)
            .map(|i| {
                input_amplitude
                    * (2.0 * std::f32::consts::PI * 1_000.0 * i as f32 / 16_000.0).sin()
            })
            .collect();

        let input_rms = rms(&input);
        let output = process_for_version(&input, AudioPipelineVersion::V2);
        let output_rms = rms(&output);
        let ratio = output_rms / input_rms;

        assert!(
            ratio > 0.9 && ratio < 1.1,
            "V2 default must preserve input RMS (no forced normalization / no pre-emphasis / \
             no 90 Hz high-pass); got ratio {ratio:.3} (input_rms={input_rms:.4}, \
             output_rms={output_rms:.4})"
        );
    }
}
