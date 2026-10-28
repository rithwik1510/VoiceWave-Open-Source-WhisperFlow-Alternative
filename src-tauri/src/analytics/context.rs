//! What every event says about the app and the PC (plan 3.3): the live
//! speech context, updated where the app already learns it, plus facts fixed
//! for the whole run (Windows major, hypervisor). Resolved when an event is
//! queued, so each one describes the app at that moment.

use crate::inference::{self, gpu_select, AsrRung};
use serde::Serialize;
use std::sync::{Mutex, PoisonError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
    None,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeechPath {
    Gpu,
    Cpu,
    /// The one-time GPU check is running or due.
    Preparing,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelFamily {
    Parakeet,
    WhisperCpp,
    FasterWhisper,
    None,
}

impl ModelFamily {
    /// The family of the selected model id (installed or not). Parakeet is a
    /// whisper.cpp id too, so it's checked first.
    pub fn of(model_id: &str) -> Self {
        if inference::is_parakeet_model(model_id) {
            Self::Parakeet
        } else if inference::is_whisper_cpp_model(model_id) {
            Self::WhisperCpp
        } else if inference::is_faster_whisper_model(model_id) {
            Self::FasterWhisper
        } else {
            Self::None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum OsMajor {
    #[serde(rename = "10")]
    Windows10,
    #[serde(rename = "11")]
    Windows11,
    #[serde(rename = "other")]
    Other,
}

impl OsMajor {
    /// From `support::os_version()`, which already names build 22000+ as
    /// Windows 11.
    pub fn current() -> Self {
        let version = crate::support::os_version();
        if version.starts_with("Windows 11") {
            Self::Windows11
        } else if version.starts_with("Windows 10") {
            Self::Windows10
        } else {
            Self::Other
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Hypervisor {
    None,
    /// Also ordinary Windows 11 PCs with Memory Integrity or WSL2, so never
    /// on its own a sign of a sandbox (plan 3.4).
    Microsoft,
    Vmware,
    Virtualbox,
    Kvm,
    Xen,
    Parallels,
    Other,
}

impl Hypervisor {
    /// CPUID leaf 1, ECX bit 31 says a hypervisor is present; leaf
    /// 0x4000_0000 then holds its 12-byte vendor string in EBX, ECX, EDX.
    #[cfg(target_arch = "x86_64")]
    pub fn detect() -> Self {
        use core::arch::x86_64::__cpuid;
        // `__cpuid` is safe: CPUID exists on every x86_64 CPU. Leaf
        // 0x4000_0000 is only read when the hypervisor bit says it's backed.
        let leaf1 = __cpuid(1);
        if leaf1.ecx & (1 << 31) == 0 {
            return Self::None;
        }
        let vendor = __cpuid(0x4000_0000);
        let mut bytes = [0u8; 12];
        bytes[..4].copy_from_slice(&vendor.ebx.to_le_bytes());
        bytes[4..8].copy_from_slice(&vendor.ecx.to_le_bytes());
        bytes[8..].copy_from_slice(&vendor.edx.to_le_bytes());
        match &bytes {
            b"Microsoft Hv" => Self::Microsoft,
            b"VMwareVMware" => Self::Vmware,
            b"VBoxVBoxVBox" => Self::Virtualbox,
            b"KVMKVMKVM\0\0\0" => Self::Kvm,
            b"XenVMMXenVMM" => Self::Xen,
            b" lrpepyh  vr" => Self::Parallels,
            _ => Self::Other,
        }
    }

    #[cfg(not(target_arch = "x86_64"))]
    pub fn detect() -> Self {
        Self::Other
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct LiveContext {
    pub gpu_vendor: GpuVendor,
    pub speech_path: SpeechPath,
    pub model_family: ModelFamily,
}

static LIVE: Mutex<LiveContext> = Mutex::new(LiveContext {
    gpu_vendor: GpuVendor::None,
    speech_path: SpeechPath::None,
    model_family: ModelFamily::None,
});

pub(super) fn live() -> LiveContext {
    *LIVE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Called with each speech engine status the controller computes (the same
/// status the UI shows). Mirrors the frontend's speech path: where the active
/// model runs once known, otherwise whether the GPU check is under way.
pub fn note_speech_engine(
    gpu_vendor: Option<gpu_select::GpuVendor>,
    running_on: Option<AsrRung>,
    preparing: bool,
) {
    let gpu_vendor = match gpu_vendor {
        Some(gpu_select::GpuVendor::Nvidia) => GpuVendor::Nvidia,
        Some(gpu_select::GpuVendor::Amd) => GpuVendor::Amd,
        Some(gpu_select::GpuVendor::Intel) => GpuVendor::Intel,
        Some(gpu_select::GpuVendor::Other) => GpuVendor::Other,
        None => GpuVendor::None,
    };
    let speech_path = match running_on {
        Some(AsrRung::Gpu) => SpeechPath::Gpu,
        Some(AsrRung::Cpu) => SpeechPath::Cpu,
        None if preparing => SpeechPath::Preparing,
        None => SpeechPath::None,
    };
    let mut live = LIVE.lock().unwrap_or_else(PoisonError::into_inner);
    live.gpu_vendor = gpu_vendor;
    live.speech_path = speech_path;
}

pub(super) fn set_model_family(model_family: ModelFamily) {
    LIVE.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .model_family = model_family;
}
