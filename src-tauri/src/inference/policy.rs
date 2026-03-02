use crate::settings::DecodeMode;
use std::collections::VecDeque;

const WINDOW_SIZE: usize = 5;
const SPEED_WINDOW_UTTERANCES: usize = 10;
const LATENCY_ENTER_SPEED_MS_DEFAULT: u64 = 2_100;
const LATENCY_RETURN_BALANCED_MS_DEFAULT: u64 = 1_850;
const LATENCY_SEVERE_MS_DEFAULT: u64 = 2_800;
const LONG_UTTERANCE_MS_DEFAULT: u64 = 9_000;
const FAILURE_GUARD_RATE: f32 = 0.25;
const ENTER_MAX_FAILURE_RATE: f32 = 0.20;

#[derive(Debug, Clone, Copy)]
struct PolicySample {
    total_ms: u64,
    failed_decode: bool,
}

#[derive(Debug, Clone)]
pub struct RuntimeDecodePolicy {
    recent: VecDeque<PolicySample>,
    speed_window_remaining: usize,
}

impl Default for RuntimeDecodePolicy {
    fn default() -> Self {
        Self {
            recent: VecDeque::with_capacity(32),
            speed_window_remaining: 0,
        }
    }
}

impl RuntimeDecodePolicy {
    pub fn select_mode(&mut self, audio_duration_ms: u64) -> DecodeMode {
        let stats = self.window_stats();

        if stats.failure_rate > FAILURE_GUARD_RATE {
            self.speed_window_remaining = 0;
            return DecodeMode::Balanced;
        }

        if self.speed_window_remaining == 0
            && stats.sample_count >= WINDOW_SIZE
            && stats.p95_total_ms > latency_enter_speed_ms()
            && stats.failure_rate <= ENTER_MAX_FAILURE_RATE
        {
            self.speed_window_remaining = SPEED_WINDOW_UTTERANCES;
        }

        if self.speed_window_remaining > 0
            && stats.sample_count >= WINDOW_SIZE
            && stats.p95_total_ms <= latency_return_balanced_ms()
        {
            self.speed_window_remaining = 0;
            return DecodeMode::Balanced;
        }

        if self.speed_window_remaining > 0 {
            let severe_latency = stats.p95_total_ms > latency_severe_ms();
            if audio_duration_ms > long_utterance_ms() && !severe_latency {
                return DecodeMode::Balanced;
            }
            self.speed_window_remaining = self.speed_window_remaining.saturating_sub(1);
            return DecodeMode::Fast;
        }

        DecodeMode::Balanced
    }

    pub fn record_success(&mut self, total_ms: u64) {
        self.push_sample(PolicySample {
            total_ms,
            failed_decode: false,
        });
    }

    pub fn record_failure(&mut self, total_ms: u64) {
        self.push_sample(PolicySample {
            total_ms,
            failed_decode: true,
        });
    }

    fn push_sample(&mut self, sample: PolicySample) {
        self.recent.push_back(sample);
        if self.recent.len() > 32 {
            self.recent.pop_front();
        }
    }

    fn window_stats(&self) -> WindowStats {
        let mut window = self
            .recent
            .iter()
            .rev()
            .take(WINDOW_SIZE)
            .copied()
            .collect::<Vec<_>>();
        if window.is_empty() {
            return WindowStats::default();
        }

        let sample_count = window.len();
        let failed_count = window.iter().filter(|row| row.failed_decode).count();
        let mut totals = window.iter().map(|row| row.total_ms).collect::<Vec<_>>();
        totals.sort_unstable();
        let p95_total_ms = percentile_u64(&totals, 0.95);
        let failure_rate = failed_count as f32 / sample_count as f32;

        window.clear();
        WindowStats {
            sample_count,
            p95_total_ms,
            failure_rate,
        }
    }
}

fn env_u64(key: &str, default_value: u64, min_value: u64, max_value: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|value| value.clamp(min_value, max_value))
        .unwrap_or(default_value)
}

fn latency_enter_speed_ms() -> u64 {
    env_u64(
        "VOICEWAVE_POLICY_ENTER_SPEED_P95_MS",
        LATENCY_ENTER_SPEED_MS_DEFAULT,
        800,
        8_000,
    )
}

fn latency_return_balanced_ms() -> u64 {
    env_u64(
        "VOICEWAVE_POLICY_RETURN_BALANCED_P95_MS",
        LATENCY_RETURN_BALANCED_MS_DEFAULT,
        700,
        8_000,
    )
}

fn latency_severe_ms() -> u64 {
    env_u64(
        "VOICEWAVE_POLICY_SEVERE_LATENCY_MS",
        LATENCY_SEVERE_MS_DEFAULT,
        1_200,
        12_000,
    )
}

fn long_utterance_ms() -> u64 {
    env_u64(
        "VOICEWAVE_POLICY_LONG_UTTERANCE_MS",
        LONG_UTTERANCE_MS_DEFAULT,
        2_500,
        60_000,
    )
}

#[derive(Debug, Clone, Copy, Default)]
struct WindowStats {
    sample_count: usize,
    p95_total_ms: u64,
    failure_rate: f32,
}

fn percentile_u64(values: &[u64], percentile: f32) -> u64 {
    if values.is_empty() {
        return 0;
    }
    let idx = ((values.len() - 1) as f32 * percentile.clamp(0.0, 1.0)).round() as usize;
    values[idx]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_enters_speed_mode_after_sustained_latency_breach() {
        let mut policy = RuntimeDecodePolicy::default();
        for _ in 0..5 {
            policy.record_success(5_800);
        }
        let mode = policy.select_mode(6_000);
        assert_eq!(mode, DecodeMode::Fast);
    }

    #[test]
    fn policy_stays_balanced_when_failure_rate_increases() {
        let mut policy = RuntimeDecodePolicy::default();
        for _ in 0..4 {
            policy.record_failure(5_500);
        }
        policy.record_success(5_600);
        let mode = policy.select_mode(6_000);
        assert_eq!(mode, DecodeMode::Balanced);
    }
}
