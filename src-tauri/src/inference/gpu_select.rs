//! Plan 019 engine selection: CPU eligibility, the model-free GPU pick behind
//! the first recommendation, and the small persistent store that remembers
//! per-machine decisions (chosen device, qualification results, per-driver
//! disable, upgrade offer).
//!
//! The hardware profile is decided once per app session. The Vulkan speech
//! worker's `--list-devices` is the source of truth for GPUs; DXGI is only a
//! cheap "is there any hardware adapter at all" pre-check so machines with
//! nothing to find never spawn the worker.

use super::asr_worker;
use crate::atomic_file::{self, StoreLoad};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// Discrete GPUs report a little less than their nominal size (an RTX 3060
/// "6 GB" lists 5.85 GiB), so the 4 GB / 2 GB rules use lenient cut-offs.
const TURBO_VRAM_BYTES: u64 = 3_584 * 1024 * 1024;
const SMALL_VRAM_BYTES: u64 = 1_792 * 1024 * 1024;
/// "Total RAM < 8 GB": 8 GB machines report ~7.8 GiB after firmware
/// reservations, so anything under 7 GiB counts as small.
const LOW_RAM_BYTES: u64 = 7 * 1024 * 1024 * 1024;
const STORE_FILE: &str = "gpu-engine.json";
const MAX_QUALIFICATION_RECORDS: usize = 12;
const DRIVER_DISABLE_TTL_MS: u64 = 7 * 24 * 60 * 60 * 1000;
/// Sessions that hit a backend failure before the GPU is disabled for that
/// driver (plan 019 failure table).
const DRIVER_DISABLE_SESSIONS: usize = 2;

/// What this CPU can run (plan 019 "CPU eligibility").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CpuSupport {
    /// No SSE4.1: below CTranslate2's minimum, nothing can transcribe.
    Unsupported,
    /// SSE4.1 without AVX2: faster-whisper only. No speech workers, no GGML
    /// models, no AI polish (the workers and the polish wheel need AVX2).
    NoAvx2,
    Full,
}

pub fn cpu_support() -> CpuSupport {
    static SUPPORT: OnceLock<CpuSupport> = OnceLock::new();
    *SUPPORT.get_or_init(|| {
        // Dev override to walk the reduced-CPU paths on a modern PC.
        match std::env::var("VOICEWAVE_CPU_SUPPORT").as_deref().map(str::trim) {
            Ok("unsupported") => CpuSupport::Unsupported,
            Ok("noavx2") => CpuSupport::NoAvx2,
            _ => detect_cpu_support(),
        }
    })
}

#[cfg(target_arch = "x86_64")]
fn detect_cpu_support() -> CpuSupport {
    if !is_x86_feature_detected!("sse4.1") {
        CpuSupport::Unsupported
    } else if is_x86_feature_detected!("avx2")
        && is_x86_feature_detected!("fma")
        && is_x86_feature_detected!("f16c")
    {
        // The workers are built for AVX2 + FMA + F16C (GGML_NATIVE=OFF).
        CpuSupport::Full
    } else {
        CpuSupport::NoAvx2
    }
}

#[cfg(not(target_arch = "x86_64"))]
fn detect_cpu_support() -> CpuSupport {
    CpuSupport::Full
}

/// A GPU the Vulkan worker can decode on, from its `--list-devices` report.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuDevice {
    pub name: String,
    pub vendor_id: u32,
    pub device_id: u32,
    pub uuid: String,
    /// Runtime match key for `load` (changes across reboots).
    pub luid: Option<String>,
    pub ggml_index: u64,
    pub driver_version: u64,
    pub discrete: bool,
    pub memory_bytes: u64,
}

impl GpuDevice {
    /// Persistent identity: survives reboots (LUIDs do not) and tells two
    /// identical cards apart (UUID).
    pub fn identity(&self) -> String {
        format!("{:04x}:{:04x}:{}", self.vendor_id, self.device_id, self.uuid)
    }

    /// Identity plus driver: the unit the per-driver disable applies to.
    pub fn driver_key(&self) -> String {
        format!("{}@{}", self.identity(), self.driver_version)
    }

    fn from_report(device: &Value) -> Option<Self> {
        let discrete = match device.get("type").and_then(Value::as_str)? {
            "discrete" => true,
            "integrated" => false,
            _ => return None,
        };
        Some(Self {
            name: device.get("name")?.as_str()?.to_string(),
            vendor_id: device.get("vendorId")?.as_u64()? as u32,
            device_id: device.get("deviceId")?.as_u64()? as u32,
            uuid: device.get("uuid")?.as_str()?.to_string(),
            luid: device
                .get("luid")
                .and_then(Value::as_str)
                .map(str::to_string),
            // Null when ggml can't use the device (old Vulkan, no fp16, ...).
            ggml_index: device.get("ggmlIndex")?.as_u64()?,
            driver_version: device
                .get("driverVersion")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            discrete,
            memory_bytes: device
                .get("memoryBytes")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        })
    }
}

/// The two sizes users choose between. Each exists in both model families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelTier {
    Small,
    LargeTurbo,
}

pub fn model_tier(model_id: &str) -> Option<ModelTier> {
    match model_id {
        "fw-small.en" | "wcpp-small.en" => Some(ModelTier::Small),
        "fw-large-v3-turbo" | "wcpp-large-v3-turbo" => Some(ModelTier::LargeTurbo),
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub struct HardwareProfile {
    pub cpu: CpuSupport,
    /// 0 when unknown.
    pub total_ram_bytes: u64,
    /// The GPU dictation runs on. `None` = a faster-whisper (CPU) machine.
    pub gpu: Option<GpuDevice>,
    pub recommended: ModelTier,
}

impl HardwareProfile {
    /// GPU-path machines see whisper.cpp (GGML) models, everyone else the
    /// faster-whisper ones; users always see just "Small" and "Large Turbo".
    /// A GPU that failed its check or is disabled for its driver stays in
    /// `gpu` (the UI names it) but is off the GPU path.
    pub fn gpu_path(&self) -> bool {
        self.gpu.as_ref().is_some_and(|device| !gpu_ruled_out(device))
    }

    pub fn model_id(&self, tier: ModelTier) -> &'static str {
        match (self.gpu_path(), tier) {
            (true, ModelTier::Small) => "wcpp-small.en",
            (true, ModelTier::LargeTurbo) => "wcpp-large-v3-turbo",
            (false, ModelTier::Small) => "fw-small.en",
            (false, ModelTier::LargeTurbo) => "fw-large-v3-turbo",
        }
    }

    pub fn recommended_model_id(&self) -> &'static str {
        // Large Turbo is only ever recommended on the GPU path.
        let tier = if self.gpu_path() {
            self.recommended
        } else {
            ModelTier::Small
        };
        self.model_id(tier)
    }
}

static PROFILE: Mutex<Option<HardwareProfile>> = Mutex::new(None);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// This machine's profile, detected on first use (may spawn the Vulkan
/// worker's `--list-devices`, ~0.3 s, 5 s deadline). Blocking.
pub fn hardware_profile() -> HardwareProfile {
    lock(&PROFILE).get_or_insert_with(detect_profile).clone()
}

pub async fn hardware_profile_async() -> HardwareProfile {
    tokio::task::spawn_blocking(hardware_profile)
        .await
        .unwrap_or_else(|_| HardwareProfile {
            cpu: cpu_support(),
            total_ram_bytes: 0,
            gpu: None,
            recommended: ModelTier::Small,
        })
}

fn detect_profile() -> HardwareProfile {
    let cpu = cpu_support();
    let total_ram_bytes = total_ram_bytes();
    // Never spawn a worker on a CPU it would crash on.
    let gpu = if cpu == CpuSupport::Full {
        pick_gpu()
    } else {
        None
    };
    let low_ram = total_ram_bytes > 0 && total_ram_bytes < LOW_RAM_BYTES;
    let recommended = match &gpu {
        Some(device) if !low_ram && device.discrete && device.memory_bytes >= TURBO_VRAM_BYTES => {
            ModelTier::LargeTurbo
        }
        _ => ModelTier::Small,
    };
    eprintln!(
        "voicewave: hardware profile cpu={cpu:?} ram={} MB gpu={} recommended={recommended:?}",
        total_ram_bytes / 1_048_576,
        gpu.as_ref()
            .map(|device| format!("'{}' ({} MB)", device.name, device.memory_bytes / 1_048_576))
            .unwrap_or_else(|| "none".to_string())
    );
    HardwareProfile {
        cpu,
        total_ram_bytes,
        gpu,
        recommended,
    }
}

/// Plan 019 device rules: a discrete GPU (any vendor) with Vulkan 1.2+ that
/// ggml supports and at least ~2 GB of video memory. Picked by the type the
/// Vulkan driver reports, never DXGI memory figures (an APU's BIOS carve-out
/// can look like dedicated memory). A device chosen before is kept while it
/// is still present. `VOICEWAVE_ASR_DEVICE=cpu` disables the GPU path,
/// `=gpu` also accepts an integrated GPU (testing).
fn pick_gpu() -> Option<GpuDevice> {
    let preference = std::env::var("VOICEWAVE_ASR_DEVICE")
        .map(|value| value.trim().to_ascii_lowercase())
        .unwrap_or_default();
    if preference == "cpu" {
        return None;
    }
    if dxgi::any_hardware_adapter() == Some(false) {
        eprintln!("voicewave: no hardware display adapter; speech runs on the CPU");
        return None;
    }
    let devices = match asr_worker::list_devices() {
        Ok(devices) => devices,
        Err(err) => {
            eprintln!("voicewave: GPU list unavailable, speech runs on the CPU: {err}");
            return None;
        }
    };
    let any_type = preference == "gpu";
    let candidates: Vec<GpuDevice> = devices
        .iter()
        .filter_map(GpuDevice::from_report)
        .filter(|device| (device.discrete && device.memory_bytes >= SMALL_VRAM_BYTES) || any_type)
        .collect();
    let remembered = read_store(|store| store.chosen_device.clone());
    let chosen = remembered
        .and_then(|identity| {
            candidates
                .iter()
                .find(|device| device.identity() == identity)
        })
        .or_else(|| {
            candidates
                .iter()
                .max_by_key(|device| (device.discrete, device.memory_bytes))
        })
        .cloned()?;
    let identity = chosen.identity();
    if read_store(|store| store.chosen_device.as_deref() != Some(identity.as_str())) {
        update_store(|store| store.chosen_device = Some(identity));
    }
    Some(chosen)
}

#[cfg(target_os = "windows")]
fn total_ram_bytes() -> u64 {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    if unsafe { GlobalMemoryStatusEx(&mut status) } != 0 {
        status.ullTotalPhys
    } else {
        0
    }
}

#[cfg(not(target_os = "windows"))]
fn total_ram_bytes() -> u64 {
    0
}

/// DXGI adapter pre-check. windows-sys carries no DXGI bindings (COM only),
/// so dxgi.dll is loaded at runtime and the two calls go through hand-rolled
/// vtables, like the mic guard's COM. Sources: Microsoft Learn,
/// "IDXGIFactory1" (EnumAdapters1 follows IDXGIObject's 4 and IDXGIFactory's
/// 5 methods), "IDXGIAdapter1::GetDesc1", "DXGI_ADAPTER_DESC1",
/// "DXGI_ADAPTER_FLAG" (SOFTWARE = 2).
#[cfg(target_os = "windows")]
mod dxgi {
    use std::ffi::c_void;
    use std::ptr;
    use windows_sys::core::GUID;
    use windows_sys::Win32::Foundation::{FreeLibrary, LUID};
    use windows_sys::Win32::System::LibraryLoader::{
        GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32,
    };

    const IID_IDXGI_FACTORY1: GUID = GUID::from_u128(0x770aae78_f26f_4dba_a829_253c83d1b387);
    const DXGI_ADAPTER_FLAG_SOFTWARE: u32 = 2;
    /// The "Microsoft Basic Render Driver" WARP adapter.
    const BASIC_RENDER_VENDOR: u32 = 0x1414;
    const BASIC_RENDER_DEVICE: u32 = 0x8c;

    type Hresult = i32;
    type CreateFactory1 = unsafe extern "system" fn(*const GUID, *mut *mut c_void) -> Hresult;
    type Release = unsafe extern "system" fn(*mut c_void) -> u32;

    #[repr(C)]
    struct AdapterDesc1 {
        description: [u16; 128],
        vendor_id: u32,
        device_id: u32,
        sub_sys_id: u32,
        revision: u32,
        dedicated_video_memory: usize,
        dedicated_system_memory: usize,
        shared_system_memory: usize,
        adapter_luid: LUID,
        flags: u32,
    }

    /// IUnknown (3), IDXGIObject (4), IDXGIFactory (5), then EnumAdapters1.
    #[repr(C)]
    struct FactoryVtbl {
        _query_interface_add_ref: [usize; 2],
        release: Release,
        _object_and_factory: [usize; 9],
        enum_adapters1: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> Hresult,
    }

    /// IUnknown (3), IDXGIObject (4), IDXGIAdapter (3), then GetDesc1.
    #[repr(C)]
    struct AdapterVtbl {
        _query_interface_add_ref: [usize; 2],
        release: Release,
        _object_and_adapter: [usize; 7],
        get_desc1: unsafe extern "system" fn(*mut c_void, *mut AdapterDesc1) -> Hresult,
    }

    /// Some(true) when any non-software adapter exists; None when DXGI could
    /// not be asked (then the Vulkan list decides).
    pub fn any_hardware_adapter() -> Option<bool> {
        let name: Vec<u16> = "dxgi.dll\0".encode_utf16().collect();
        unsafe {
            let module = LoadLibraryExW(name.as_ptr(), ptr::null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32);
            if module.is_null() {
                return None;
            }
            let result = enumerate(module);
            FreeLibrary(module);
            result
        }
    }

    unsafe fn enumerate(module: *mut c_void) -> Option<bool> {
        let create: CreateFactory1 =
            std::mem::transmute(GetProcAddress(module, b"CreateDXGIFactory1\0".as_ptr())?);
        let mut factory: *mut c_void = ptr::null_mut();
        if create(&IID_IDXGI_FACTORY1, &mut factory) < 0 || factory.is_null() {
            return None;
        }
        let factory_vtbl = *(factory as *const *const FactoryVtbl);
        let mut found = false;
        // EnumAdapters1 fails with DXGI_ERROR_NOT_FOUND past the last one.
        for index in 0..16 {
            let mut adapter: *mut c_void = ptr::null_mut();
            if ((*factory_vtbl).enum_adapters1)(factory, index, &mut adapter) < 0 || adapter.is_null() {
                break;
            }
            let adapter_vtbl = *(adapter as *const *const AdapterVtbl);
            let mut desc: AdapterDesc1 = std::mem::zeroed();
            let described = ((*adapter_vtbl).get_desc1)(adapter, &mut desc) >= 0;
            ((*adapter_vtbl).release)(adapter);
            let software = desc.flags & DXGI_ADAPTER_FLAG_SOFTWARE != 0
                || (desc.vendor_id == BASIC_RENDER_VENDOR && desc.device_id == BASIC_RENDER_DEVICE);
            if described && !software {
                found = true;
                break;
            }
        }
        ((*factory_vtbl).release)(factory);
        Some(found)
    }
}

#[cfg(not(target_os = "windows"))]
mod dxgi {
    pub fn any_hardware_adapter() -> Option<bool> {
        None
    }
}

// ------------------------------------------------------------------ store ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum QualOutcome {
    /// GPU path on; cached for the key.
    Passed,
    /// Wrong transcript, GPU not actually used, crash: cached for the key.
    Failed,
    /// Correct but slow, out of memory, cold-cache timeout: CPU this
    /// session, retried next session. Never a cache hit, but the third in a
    /// row for a key is recorded as `Failed` instead.
    Retry,
}

/// One qualification run (plan 019 "Qualification"), kept for the cache and
/// for local diagnostics.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QualificationRecord {
    /// Device identity, driver version, model id + hash, worker build
    /// fingerprint and decode configuration. Not the app version.
    pub key: String,
    pub outcome: QualOutcome,
    pub model_id: String,
    pub device_name: String,
    pub at_utc_ms: u64,
    pub detail: String,
    pub load_ms: u64,
    pub short_clip_ms: Option<u64>,
    pub long_clip_ms: Option<u64>,
    /// faster-whisper on the CPU for the same clips (upgraders only).
    pub cpu_short_clip_ms: Option<u64>,
    pub cpu_long_clip_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackendFailure {
    driver_key: String,
    session: u64,
    at_utc_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DriverDisable {
    driver_key: String,
    at_utc_ms: u64,
    app_version: String,
    worker_fingerprint: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct EngineStore {
    /// Identity of the GPU picked for this machine.
    chosen_device: Option<String>,
    qualifications: Vec<QualificationRecord>,
    backend_failures: Vec<BackendFailure>,
    driver_disables: Vec<DriverDisable>,
    /// The one-time "your graphics card can make dictation faster" notice
    /// was accepted or declined.
    upgrade_offer_answered: bool,
    /// GGML model the user accepted the upgrade to; switched to once it is
    /// installed and qualified. Survives restarts.
    pending_upgrade: Option<String>,
}

static STORE: Mutex<Option<EngineStore>> = Mutex::new(None);
static STORE_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();

fn store_path() -> Option<PathBuf> {
    STORE_PATH
        .get_or_init(|| {
            // Unit tests keep the store in memory, never in the user's app data.
            if cfg!(test) {
                return None;
            }
            ProjectDirs::from("com", "voicewave", "localcore")
                .map(|dirs| dirs.config_dir().join(STORE_FILE))
        })
        .clone()
}

fn with_loaded<T>(f: impl FnOnce(&mut EngineStore) -> T) -> T {
    let mut guard = lock(&STORE);
    let store = guard.get_or_insert_with(|| {
        let Some(path) = store_path() else {
            return EngineStore::default();
        };
        match atomic_file::load_with_recovery(&path, "gpu engine", |raw| {
            serde_json::from_str::<EngineStore>(raw)
        }) {
            StoreLoad::Loaded(store) | StoreLoad::Recovered(store) => store,
            StoreLoad::Missing | StoreLoad::Reset => EngineStore::default(),
        }
    });
    f(store)
}

fn read_store<T>(f: impl FnOnce(&EngineStore) -> T) -> T {
    with_loaded(|store| f(store))
}

fn update_store(f: impl FnOnce(&mut EngineStore)) {
    with_loaded(|store| {
        f(store);
        let Some(path) = store_path() else {
            return;
        };
        let written = serde_json::to_vec_pretty(store)
            .map_err(std::io::Error::other)
            .and_then(|raw| atomic_file::atomic_write(&path, &raw));
        if let Err(err) = written {
            eprintln!("voicewave: could not save the gpu engine store: {err}");
        }
    });
}

fn now_utc_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|age| age.as_millis() as u64)
        .unwrap_or(0)
}

/// Identifies this app session in the per-driver failure log.
fn session_id() -> u64 {
    static SESSION: OnceLock<u64> = OnceLock::new();
    *SESSION.get_or_init(now_utc_ms)
}

/// Cached pass/fail for a qualification key (`Retry` is never a cache hit).
pub fn cached_qualification(key: &str) -> Option<QualOutcome> {
    read_store(|store| {
        store
            .qualifications
            .iter()
            .rev()
            .find(|record| record.key == key && record.outcome != QualOutcome::Retry)
            .map(|record| record.outcome)
    })
}

/// Detail prefix of `Retry` records the check could not finish (GPU memory
/// busy, interrupted); they say nothing about the GPU, so the retry cap
/// skips them.
pub const INCONCLUSIVE_PREFIX: &str = "inconclusive (GPU busy or check interrupted): ";

/// Counted `Retry` records for `key` since its last pass or fail.
pub fn consecutive_retries(key: &str) -> usize {
    read_store(|store| {
        store
            .qualifications
            .iter()
            .rev()
            .filter(|record| record.key == key)
            .take_while(|record| record.outcome == QualOutcome::Retry)
            .filter(|record| !record.detail.starts_with(INCONCLUSIVE_PREFIX))
            .count()
    })
}

/// The GPU path is off for this device: its driver is disabled, or a
/// qualification failed on this device, driver and worker build and none
/// passed there. Any model counts: a broken Vulkan path fails Small and
/// Large Turbo alike, while a pass proves the path works (a model-specific
/// fail is then handled per key at decode time). A driver or worker update
/// opens the path again, like the driver disable. Matches the
/// `identity|driver N|...|worker F|...` layout of
/// `asr_worker::qualification_key`.
fn gpu_ruled_out(device: &GpuDevice) -> bool {
    if driver_disabled(device) {
        return true;
    }
    let prefix = format!("{}|driver {}|", device.identity(), device.driver_version);
    let records: Vec<(String, QualOutcome)> = read_store(|store| {
        store
            .qualifications
            .iter()
            .filter(|record| record.outcome != QualOutcome::Retry && record.key.starts_with(&prefix))
            .map(|record| (record.key.clone(), record.outcome))
            .collect()
    });
    // Skip hashing the worker EXE on the common no-record path.
    if records.is_empty() {
        return false;
    }
    let worker = format!("|worker {}|", asr_worker::worker_fingerprint());
    let outcomes: Vec<QualOutcome> = records
        .into_iter()
        .filter(|(key, _)| key.contains(&worker))
        .map(|(_, outcome)| outcome)
        .collect();
    outcomes.contains(&QualOutcome::Failed) && !outcomes.contains(&QualOutcome::Passed)
}

pub fn latest_qualification(key: &str) -> Option<QualificationRecord> {
    read_store(|store| {
        store
            .qualifications
            .iter()
            .rev()
            .find(|record| record.key == key)
            .cloned()
    })
}

pub fn record_qualification(record: QualificationRecord) {
    update_store(|store| {
        store.qualifications.push(record);
        let excess = store
            .qualifications
            .len()
            .saturating_sub(MAX_QUALIFICATION_RECORDS);
        store.qualifications.drain(..excess);
    });
}

/// Note a GPU backend failure the app observed during a dictation. Returns
/// true when this is the second session with one on this driver, i.e. the GPU
/// is now disabled for it.
pub fn record_backend_failure(device: &GpuDevice) -> bool {
    let driver_key = device.driver_key();
    let session = session_id();
    let now = now_utc_ms();
    let mut disabled = false;
    update_store(|store| {
        store
            .backend_failures
            .retain(|failure| now.saturating_sub(failure.at_utc_ms) < DRIVER_DISABLE_TTL_MS);
        if !store
            .backend_failures
            .iter()
            .any(|failure| failure.driver_key == driver_key && failure.session == session)
        {
            store.backend_failures.push(BackendFailure {
                driver_key: driver_key.clone(),
                session,
                at_utc_ms: now,
            });
        }
        let sessions = store
            .backend_failures
            .iter()
            .filter(|failure| failure.driver_key == driver_key)
            .count();
        if sessions >= DRIVER_DISABLE_SESSIONS {
            store
                .backend_failures
                .retain(|failure| failure.driver_key != driver_key);
            store
                .driver_disables
                .retain(|disable| disable.driver_key != driver_key);
            store.driver_disables.push(DriverDisable {
                driver_key: driver_key.clone(),
                at_utc_ms: now,
                app_version: env!("CARGO_PKG_VERSION").to_string(),
                worker_fingerprint: asr_worker::worker_fingerprint(),
            });
            disabled = true;
        }
    });
    disabled
}

/// GPU disabled for this device's driver after failures in two sessions.
/// Expires after 7 days or on any app or worker update.
pub fn driver_disabled(device: &GpuDevice) -> bool {
    let driver_key = device.driver_key();
    let now = now_utc_ms();
    let disable = read_store(|store| {
        store
            .driver_disables
            .iter()
            .find(|disable| disable.driver_key == driver_key)
            .cloned()
    });
    disable.is_some_and(|disable| {
        now.saturating_sub(disable.at_utc_ms) < DRIVER_DISABLE_TTL_MS
            && disable.app_version == env!("CARGO_PKG_VERSION")
            && disable.worker_fingerprint == asr_worker::worker_fingerprint()
    })
}

pub fn upgrade_offer_answered() -> bool {
    read_store(|store| store.upgrade_offer_answered)
}

pub fn pending_upgrade() -> Option<String> {
    read_store(|store| store.pending_upgrade.clone())
}

/// Record the user's answer to the upgrade notice; `accepted` names the
/// model to switch to once it is installed and qualified.
pub fn answer_upgrade_offer(accepted: Option<String>) {
    update_store(|store| {
        store.upgrade_offer_answered = true;
        store.pending_upgrade = accepted;
    });
}

pub fn clear_pending_upgrade() {
    update_store(|store| store.pending_upgrade = None);
}

/// Everything local diagnostics export about engine selection.
pub fn diagnostics_snapshot() -> Value {
    let profile = lock(&PROFILE).clone();
    let store = read_store(|store| store.clone());
    serde_json::json!({
        "cpuSupport": profile.as_ref().map(|profile| profile.cpu),
        "totalRamMb": profile.as_ref().map(|profile| profile.total_ram_bytes / 1_048_576),
        "gpu": profile.as_ref().and_then(|profile| profile.gpu.clone()),
        "recommended": profile.as_ref().map(|profile| profile.recommended),
        "store": store,
    })
}
