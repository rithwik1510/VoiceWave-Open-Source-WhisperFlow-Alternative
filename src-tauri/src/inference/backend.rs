use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use whisper_rs::WhisperContextParameters;

const DEFAULT_GPU_DEVICE_ID: i32 = 0;
const GPU_FAILURE_LOCK_THRESHOLD: u8 = 2;
static GPU_SESSION_FAILURES: AtomicU8 = AtomicU8::new(0);
static GPU_SESSION_CPU_LOCK: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeBackend {
    Cpu,
    Cuda,
    Vulkan,
}

impl RuntimeBackend {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Cuda => "cuda",
            Self::Vulkan => "vulkan",
        }
    }
}

pub(crate) fn backend_policy_version(model_id: &str) -> String {
    let backend = preferred_backend_for_model(model_id);
    format!(
        "{}:{}:{}:{}:{}:{}",
        backend.as_str(),
        cuda_backend_compiled(),
        auto_gpu_enabled(),
        force_gpu_enabled(),
        gpu_device_id(),
        gpu_session_cpu_locked()
    )
}

pub(crate) fn preferred_backend_for_model(model_id: &str) -> RuntimeBackend {
    if force_cpu_enabled() {
        return RuntimeBackend::Cpu;
    }
    if gpu_session_cpu_locked() {
        return RuntimeBackend::Cpu;
    }
    if !any_gpu_backend_compiled() {
        return RuntimeBackend::Cpu;
    }
    if force_gpu_enabled() {
        return preferred_gpu_backend();
    }
    if !auto_gpu_enabled() {
        return RuntimeBackend::Cpu;
    }
    if is_gpu_preferred_model(model_id) {
        return preferred_gpu_backend();
    }
    RuntimeBackend::Cpu
}

pub(crate) fn context_params_for_backend(
    backend: RuntimeBackend,
) -> WhisperContextParameters<'static> {
    let mut params = WhisperContextParameters::default();
    let use_gpu = match backend {
        RuntimeBackend::Cuda => cuda_backend_compiled(),
        RuntimeBackend::Vulkan => vulkan_backend_compiled(),
        RuntimeBackend::Cpu => false,
    };
    params.use_gpu(use_gpu);
    params.gpu_device(gpu_device_id());
    params
}

pub(crate) fn cuda_backend_compiled() -> bool {
    cfg!(feature = "whisper-cuda")
}

pub(crate) fn vulkan_backend_compiled() -> bool {
    cfg!(feature = "whisper-vulkan")
}

fn any_gpu_backend_compiled() -> bool {
    cuda_backend_compiled() || vulkan_backend_compiled()
}

// Prefer Vulkan: works on NVIDIA/AMD/Intel with no DLL bundling. CUDA is
// available only when explicitly compiled with whisper-cuda feature.
fn preferred_gpu_backend() -> RuntimeBackend {
    if vulkan_backend_compiled() {
        RuntimeBackend::Vulkan
    } else {
        RuntimeBackend::Cuda
    }
}

pub(crate) fn gpu_session_cpu_locked() -> bool {
    GPU_SESSION_CPU_LOCK.load(Ordering::Relaxed)
}

pub(crate) fn note_gpu_runtime_failure() -> bool {
    let failures = GPU_SESSION_FAILURES
        .fetch_add(1, Ordering::Relaxed)
        .saturating_add(1);
    if failures >= GPU_FAILURE_LOCK_THRESHOLD {
        GPU_SESSION_CPU_LOCK.store(true, Ordering::Relaxed);
    }
    gpu_session_cpu_locked()
}

fn is_gpu_preferred_model(model_id: &str) -> bool {
    let normalized = model_id.trim().to_ascii_lowercase();
    !(normalized.starts_with("tiny") || normalized.starts_with("base"))
}

fn force_cpu_enabled() -> bool {
    env_flag("VOICEWAVE_FORCE_CPU", false)
}

fn force_gpu_enabled() -> bool {
    env_flag("VOICEWAVE_FORCE_GPU", false)
}

fn auto_gpu_enabled() -> bool {
    env_flag("VOICEWAVE_AUTO_GPU", true)
}

fn gpu_device_id() -> i32 {
    std::env::var("VOICEWAVE_GPU_DEVICE")
        .ok()
        .and_then(|value| value.trim().parse::<i32>().ok())
        .filter(|value| *value >= 0)
        .unwrap_or(DEFAULT_GPU_DEVICE_ID)
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
    use super::*;

    #[test]
    fn tiny_and_base_default_to_cpu() {
        assert_eq!(preferred_backend_for_model("tiny.en"), RuntimeBackend::Cpu);
        assert_eq!(preferred_backend_for_model("base.en"), RuntimeBackend::Cpu);
    }

    #[test]
    fn policy_version_includes_backend_name() {
        let version = backend_policy_version("small.en");
        assert!(version.contains("cpu") || version.contains("cuda") || version.contains("vulkan"));
    }

    #[test]
    fn vulkan_backend_string_is_vulkan() {
        assert_eq!(RuntimeBackend::Vulkan.as_str(), "vulkan");
    }

    #[cfg(feature = "whisper-vulkan")]
    #[test]
    fn small_prefers_vulkan_when_vulkan_compiled() {
        GPU_SESSION_CPU_LOCK.store(false, std::sync::atomic::Ordering::Relaxed);
        GPU_SESSION_FAILURES.store(0, std::sync::atomic::Ordering::Relaxed);
        let backend = preferred_backend_for_model("wcpp-small.en");
        assert_eq!(backend, RuntimeBackend::Vulkan);
    }

    #[test]
    fn gpu_failure_guard_locks_after_threshold() {
        GPU_SESSION_FAILURES.store(0, Ordering::Relaxed);
        GPU_SESSION_CPU_LOCK.store(false, Ordering::Relaxed);
        assert!(!gpu_session_cpu_locked());
        assert!(!note_gpu_runtime_failure());
        assert!(note_gpu_runtime_failure());
    }
}
