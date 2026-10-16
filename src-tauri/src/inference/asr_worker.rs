//! whisper.cpp speech workers (plan 019): process transport, deadlines, and
//! the GPU/CPU rung bookkeeping behind `InferenceBackend::AsrWorker`.
//!
//! Two executables are built from `src-tauri/windows/asr-worker/main.cpp`
//! (the JSON-lines protocol is documented in its header):
//!   voicewave-asr-vk.exe   Vulkan + CPU: GPU dictation and the device list
//!   voicewave-asr-cpu.exe  CPU only, no Vulkan anywhere: the CPU rung
//!
//! Each worker keeps one model resident. Every exchange runs under a deadline
//! that also covers writing the request (stdin is fed by a writer thread, so a
//! worker that stops reading cannot block us untimed). A missed deadline, an
//! exit, or any failed request kills and reaps the worker before anything else
//! starts; the next use respawns it.

use super::faster_whisper::{
    encode_pcm16_base64, kill_process_by_pid, FasterWhisperDecodeOutput,
    FasterWhisperRequestOverrides,
};
use super::InferenceError;
use crate::settings::DecodeMode;
use directories::ProjectDirs;
use flate2::{write::ZlibEncoder, Compression};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::fmt;
use std::io::{BufRead, BufReader, Write};
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc::{self, RecvTimeoutError},
    Arc, Mutex, MutexGuard, TryLockError,
};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

// Per-phase deadlines (plan 019 "Deadlines").
const SPAWN_READY_DEADLINE: Duration = Duration::from_secs(10);
const LIST_DEVICES_DEADLINE: Duration = Duration::from_secs(5);
const LOAD_GPU_DEADLINE: Duration = Duration::from_secs(30);
/// First GPU load on a machine (and after a driver or worker update) waits
/// for the driver to compile ggml's shaders: 25-41 s measured on an RTX 3060.
/// Missing this deadline is "retry later", never a GPU failure.
const LOAD_GPU_FIRST_USE_DEADLINE: Duration = Duration::from_secs(120);
const LOAD_CPU_DEADLINE: Duration = Duration::from_secs(20);
/// Upper bound on waiting for a killed worker to actually exit.
const REAP_WAIT: Duration = Duration::from_secs(5);
/// How often a waiting request checks the user's cancel token.
const POLL_SLICE: Duration = Duration::from_millis(50);
const STDERR_TAIL_LINES: usize = 40;
/// Backend failures in one session after which the GPU is off until restart.
const SESSION_GPU_FAILURE_LIMIT: u32 = 2;
const GPU_READY_MARKER: &str = "asr-gpu-ready.txt";

fn transcribe_deadline(rung: Rung, audio_ms: u64) -> Duration {
    match rung {
        Rung::Gpu => Duration::from_millis(10_000 + audio_ms),
        Rung::Cpu => Duration::from_millis(15_000 + audio_ms.saturating_mul(2)),
    }
}

fn load_deadline(rung: Rung, first_gpu_use: bool) -> Duration {
    match (rung, first_gpu_use) {
        (Rung::Gpu, true) => LOAD_GPU_FIRST_USE_DEADLINE,
        (Rung::Gpu, false) => LOAD_GPU_DEADLINE,
        (Rung::Cpu, _) => LOAD_CPU_DEADLINE,
    }
}

/// Worst case for one decode on `rung`: device pick, spawn (or waiting for a
/// load already in flight), load, primary + confidence-retry decodes, reaps.
/// Only a backstop for the controller's outer timeout; the per-phase
/// deadlines above always fire first.
pub(crate) fn rung_budget(rung: Rung, audio_ms: u64) -> Duration {
    let setup = match rung {
        Rung::Gpu => LIST_DEVICES_DEADLINE + SPAWN_READY_DEADLINE + LOAD_GPU_DEADLINE,
        Rung::Cpu => SPAWN_READY_DEADLINE + LOAD_CPU_DEADLINE,
    };
    setup + transcribe_deadline(rung, audio_ms) * 2 + REAP_WAIT * 2
}

/// Which worker a decode runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rung {
    Gpu,
    Cpu,
}

impl Rung {
    fn exe_name(self) -> &'static str {
        match self {
            Rung::Gpu => "voicewave-asr-vk.exe",
            Rung::Cpu => "voicewave-asr-cpu.exe",
        }
    }

    fn dir_name(self) -> &'static str {
        match self {
            Rung::Gpu => "vk",
            Rung::Cpu => "cpu",
        }
    }

    /// Engine label for diagnostics.
    pub fn engine(self) -> &'static str {
        match self {
            Rung::Gpu => "whisper.cpp-vulkan",
            Rung::Cpu => "whisper.cpp-cpu",
        }
    }

    pub fn backend(self) -> &'static str {
        match self {
            Rung::Gpu => "vulkan",
            Rung::Cpu => "cpu",
        }
    }
}

/// Failure classes from the worker protocol (plan 019 "Runtime failure
/// handling"). Deadline misses and exits are `Backend`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorClass {
    /// Model file unreadable or corrupt: re-download, not a GPU fault.
    Artifact,
    /// Out of GPU or system memory: CPU for this dictation, never counted.
    Resource,
    /// Device fault, crash, hang.
    Backend,
    /// Malformed reply or id mismatch: a worker bug.
    Protocol,
}

impl ErrorClass {
    fn parse(value: &str) -> Self {
        match value {
            "artifact" => Self::Artifact,
            "resource" => Self::Resource,
            "backend" => Self::Backend,
            _ => Self::Protocol,
        }
    }
}

impl fmt::Display for ErrorClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Artifact => "artifact",
            Self::Resource => "resource",
            Self::Backend => "backend",
            Self::Protocol => "protocol",
        })
    }
}

/// Internal outcome of a failed worker operation; converted to
/// `InferenceError` at the module boundary.
enum Fault {
    Cancelled,
    /// The GPU worker is still preparing (first-use load in flight). The
    /// dictation steps down; not a failure.
    NotReady(String),
    Worker {
        class: ErrorClass,
        reason: String,
        timed_out: bool,
    },
}

impl Fault {
    fn worker(class: ErrorClass, reason: impl Into<String>) -> Self {
        Fault::Worker {
            class,
            reason: reason.into(),
            timed_out: false,
        }
    }
}

impl From<Fault> for InferenceError {
    fn from(fault: Fault) -> Self {
        match fault {
            Fault::Cancelled => InferenceError::Cancelled,
            Fault::NotReady(reason) => InferenceError::DecodeFailed {
                model_id: "speech-worker".to_string(),
                reason,
            },
            Fault::Worker { class, reason, .. } => InferenceError::AsrWorker { class, reason },
        }
    }
}

type StderrTail = Arc<Mutex<VecDeque<String>>>;

struct Loaded {
    model_path: PathBuf,
}

struct WorkerProcess {
    rung: Rung,
    child: Child,
    /// Lines for the writer thread. Dropping it closes the worker's stdin.
    requests: mpsc::Sender<String>,
    /// stdout lines from the reader thread; disconnected = the worker exited.
    responses: mpsc::Receiver<String>,
    stderr_tail: StderrTail,
    loaded: Option<Loaded>,
}

/// Windows never kills a child with its parent, so every handle dropped for
/// any reason kills and reaps its process.
impl Drop for WorkerProcess {
    fn drop(&mut self) {
        let pid = self.child.id();
        let _ = self.child.kill();
        let deadline = Instant::now() + REAP_WAIT;
        loop {
            match self.child.try_wait() {
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20))
                }
                Ok(None) => {
                    eprintln!(
                        "voicewave: asr worker pid {pid} still running {} ms after kill",
                        REAP_WAIT.as_millis()
                    );
                    break;
                }
                _ => break,
            }
        }
        if let Ok(mut pids) = LIVE_PIDS.lock() {
            pids.retain(|value| *value != pid);
        }
    }
}

impl WorkerProcess {
    fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    fn stderr_text(&self) -> String {
        let Ok(tail) = self.stderr_tail.lock() else {
            return String::new();
        };
        let skip = tail.len().saturating_sub(3);
        tail.iter()
            .skip(skip)
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(" | ")
    }

    /// Next stdout line before `until`. `Ok(None)` = nothing yet this slice.
    fn next_line(&self, until: Instant, what: &str) -> Result<Option<String>, Fault> {
        let now = Instant::now();
        if now >= until {
            return Err(Fault::Worker {
                class: ErrorClass::Backend,
                reason: format!("{} worker missed its {what} deadline", self.rung.dir_name()),
                timed_out: true,
            });
        }
        match self.responses.recv_timeout(POLL_SLICE.min(until - now)) {
            Ok(line) => Ok(Some(line)),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err(self.exited(what)),
        }
    }

    fn exited(&self, what: &str) -> Fault {
        // Give the stderr drain a moment to capture the last lines.
        std::thread::sleep(Duration::from_millis(100));
        Fault::worker(
            ErrorClass::Backend,
            format!(
                "{} worker exited during {what}. stderr: {}",
                self.rung.dir_name(),
                self.stderr_text()
            ),
        )
    }

    fn wait_ready(&mut self) -> Result<(), Fault> {
        let until = Instant::now() + SPAWN_READY_DEADLINE;
        let line = loop {
            if let Some(line) = self.next_line(until, "start")? {
                break line;
            }
        };
        let ready: Value = serde_json::from_str(line.trim())
            .map_err(|_| Fault::worker(ErrorClass::Protocol, format!("bad ready line: {line}")))?;
        if ready.get("ready") != Some(&Value::Bool(true))
            || ready.get("protocol").and_then(Value::as_u64) != Some(1)
        {
            return Err(Fault::worker(
                ErrorClass::Protocol,
                format!("unexpected ready line: {line}"),
            ));
        }
        Ok(())
    }

    /// Send `body` (a request without id) and wait for its reply under
    /// `deadline`. The deadline starts before the write, so a worker that
    /// stops reading stdin is caught too. A user cancel sends `cancel` and
    /// keeps waiting (same deadline) for the cancelled reply.
    fn request(
        &mut self,
        mut body: Value,
        deadline: Duration,
        cancel: Option<&CancellationToken>,
    ) -> Result<Value, Fault> {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        let until = Instant::now() + deadline;
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let command = body["command"].as_str().unwrap_or("request").to_string();
        body["id"] = json!(id);
        if self.requests.send(body.to_string()).is_err() {
            return Err(self.exited(&command));
        }
        let mut cancel_id = None;
        loop {
            if cancel_id.is_none() && cancel.is_some_and(CancellationToken::is_cancelled) {
                let cid = NEXT_ID.fetch_add(1, Ordering::Relaxed);
                let _ = self
                    .requests
                    .send(json!({"id": cid, "command": "cancel", "targetId": id}).to_string());
                cancel_id = Some(cid);
            }
            let Some(line) = self.next_line(until, &command)? else {
                continue;
            };
            let reply: Value = serde_json::from_str(line.trim()).map_err(|_| {
                Fault::worker(ErrorClass::Protocol, format!("unparseable reply: {line}"))
            })?;
            let reply_id = reply.get("id").and_then(Value::as_u64);
            if reply_id.is_some() && reply_id == cancel_id {
                continue; // the cancel acknowledgement
            }
            if reply_id != Some(id) {
                return Err(Fault::worker(
                    ErrorClass::Protocol,
                    format!("reply id mismatch (expected {id}): {line}"),
                ));
            }
            if reply.get("ok") == Some(&Value::Bool(true)) {
                return Ok(reply);
            }
            if reply.get("cancelled") == Some(&Value::Bool(true)) {
                return Err(Fault::Cancelled);
            }
            let class = reply
                .get("errorClass")
                .and_then(Value::as_str)
                .map(ErrorClass::parse)
                .unwrap_or(ErrorClass::Protocol);
            let reason = reply
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("worker request failed")
                .to_string();
            return Err(Fault::worker(class, reason));
        }
    }
}

static GPU_SLOT: Mutex<Option<WorkerProcess>> = Mutex::new(None);
static CPU_SLOT: Mutex<Option<WorkerProcess>> = Mutex::new(None);
/// Pids of every live worker, so app exit can reach a worker whose slot is
/// held by an in-flight request.
static LIVE_PIDS: Mutex<Vec<u32>> = Mutex::new(Vec::new());
static SESSION: Mutex<Session> = Mutex::new(Session {
    placement: None,
    gpu_failures: 0,
});

struct Session {
    /// Decided once per app session from the vk worker's device list.
    placement: Option<Placement>,
    /// Backend failures of the GPU rung this session.
    gpu_failures: u32,
}

#[derive(Debug, Clone)]
enum Placement {
    Gpu(GpuDevice),
    Cpu,
}

#[derive(Debug, Clone)]
struct GpuDevice {
    name: String,
    discrete: bool,
    luid: Option<String>,
    ggml_index: u64,
    driver_version: u64,
}

fn slot(rung: Rung) -> &'static Mutex<Option<WorkerProcess>> {
    match rung {
        Rung::Gpu => &GPU_SLOT,
        Rung::Cpu => &CPU_SLOT,
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Lock a worker slot, waiting at most `wait` for a load or decode in flight.
fn lock_slot(rung: Rung, wait: Duration) -> Option<MutexGuard<'static, Option<WorkerProcess>>> {
    let until = Instant::now() + wait;
    loop {
        match slot(rung).try_lock() {
            Ok(guard) => return Some(guard),
            Err(TryLockError::Poisoned(poisoned)) => return Some(poisoned.into_inner()),
            Err(TryLockError::WouldBlock) if Instant::now() < until => {
                std::thread::sleep(Duration::from_millis(25))
            }
            Err(TryLockError::WouldBlock) => return None,
        }
    }
}

/// Run `op` on the slot's worker; any failure except a user cancel drops
/// (kills and reaps) it so the next use starts clean.
fn with_worker<T>(
    guard: &mut Option<WorkerProcess>,
    op: impl FnOnce(&mut WorkerProcess) -> Result<T, Fault>,
) -> Result<T, Fault> {
    let Some(worker) = guard.as_mut() else {
        return Err(Fault::worker(ErrorClass::Backend, "no worker in slot"));
    };
    let result = op(worker);
    if matches!(result, Err(ref fault) if !matches!(fault, Fault::Cancelled)) {
        *guard = None;
    }
    result
}

fn ensure_spawned(guard: &mut Option<WorkerProcess>, rung: Rung) -> Result<(), Fault> {
    if let Some(worker) = guard.as_mut() {
        if worker.alive() {
            return Ok(());
        }
        // Found dead while idle (sleep/resume, driver reset): respawn
        // lazily, not a failure.
        eprintln!(
            "voicewave: {} speech worker found dead while idle; respawning",
            rung.dir_name()
        );
        *guard = None;
    }
    *guard = Some(spawn(rung)?);
    Ok(())
}

fn spawn(rung: Rung) -> Result<WorkerProcess, Fault> {
    let exe = resolve_worker_exe(rung)?;
    let mut command = Command::new(&exe);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(target_os = "windows")]
    command.creation_flags(CREATE_NO_WINDOW);
    let mut child = command.spawn().map_err(|err| {
        Fault::worker(
            ErrorClass::Backend,
            format!("failed to start {}: {err}", exe.display()),
        )
    })?;
    lock(&LIVE_PIDS).push(child.id());
    let stdio = (child.stdin.take(), child.stdout.take(), child.stderr.take());
    let (request_tx, request_rx) = mpsc::channel::<String>();
    let (response_tx, response_rx) = mpsc::channel::<String>();
    let mut worker = WorkerProcess {
        rung,
        child,
        requests: request_tx,
        responses: response_rx,
        stderr_tail: Arc::new(Mutex::new(VecDeque::new())),
        loaded: None,
    };
    // From here on, dropping `worker` kills and reaps the process.
    let (Some(mut stdin), Some(stdout), Some(stderr)) = stdio else {
        return Err(Fault::worker(
            ErrorClass::Backend,
            "worker stdio unavailable",
        ));
    };
    std::thread::spawn(move || {
        for line in request_rx {
            let written = stdin
                .write_all(line.as_bytes())
                .and_then(|_| stdin.write_all(b"\n"))
                .and_then(|_| stdin.flush());
            if written.is_err() {
                break;
            }
        }
    });
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if response_tx.send(line).is_err() {
                break;
            }
        }
    });
    let tail = Arc::clone(&worker.stderr_tail);
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines() {
            let Ok(line) = line else { break };
            let mut tail = lock(&tail);
            if tail.len() >= STDERR_TAIL_LINES {
                tail.pop_front();
            }
            tail.push_back(line);
        }
    });
    worker.wait_ready()?;
    Ok(worker)
}

fn resolve_worker_exe(rung: Rung) -> Result<PathBuf, Fault> {
    let relative = Path::new("asr-worker")
        .join(rung.dir_name())
        .join(rung.exe_name());
    let mut candidates = Vec::new();
    if let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
    {
        candidates.push(exe_dir.join(&relative));
        candidates.push(exe_dir.join("resources").join(&relative));
    }
    // Dev builds: the output of scripts/asr-worker/build-asr-worker.ps1.
    candidates.push(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("windows")
            .join("asr-worker")
            .join("bin")
            .join(rung.dir_name())
            .join(rung.exe_name()),
    );
    candidates
        .into_iter()
        .find(|path| path.exists())
        .ok_or_else(|| {
            Fault::worker(
                ErrorClass::Backend,
                format!("{} is missing from this install", rung.exe_name()),
            )
        })
}

/// Where the next dictation decodes first, plus a step-down reason when the
/// session is locked to CPU. Phase 2 plugs CPU eligibility, qualification
/// and the persistent per-driver disable in here.
fn primary_rung_blocking() -> (Rung, Option<String>) {
    let mut session = lock(&SESSION);
    if session.gpu_failures >= SESSION_GPU_FAILURE_LIMIT {
        return (
            Rung::Cpu,
            Some("session-cpu-lock: repeated GPU failures this session".to_string()),
        );
    }
    let placement = session.placement.get_or_insert_with(choose_placement);
    match placement {
        Placement::Gpu(_) => (Rung::Gpu, None),
        Placement::Cpu => (Rung::Cpu, None),
    }
}

/// Phase 1 device rule: the first discrete GPU ggml can use, else CPU.
/// `VOICEWAVE_ASR_DEVICE=cpu|gpu` overrides for testing (`gpu` also accepts
/// an integrated GPU).
fn choose_placement() -> Placement {
    let preference = std::env::var("VOICEWAVE_ASR_DEVICE")
        .map(|value| value.trim().to_ascii_lowercase())
        .unwrap_or_default();
    if preference == "cpu" {
        return Placement::Cpu;
    }
    let any_gpu = preference == "gpu";
    let mut guard = lock(&GPU_SLOT);
    let listed = ensure_spawned(&mut guard, Rung::Gpu).and_then(|_| {
        with_worker(&mut guard, |worker| {
            worker.request(
                json!({"command": "list_devices"}),
                LIST_DEVICES_DEADLINE,
                None,
            )
        })
    });
    let reply = match listed {
        Ok(reply) => reply,
        Err(fault) => {
            *guard = None;
            eprintln!(
                "voicewave: speech worker device list unavailable, using CPU: {}",
                InferenceError::from(fault)
            );
            return Placement::Cpu;
        }
    };
    let devices = reply
        .get("devices")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let pick = |want: &str| {
        devices.iter().find_map(|device| {
            let ggml_index = device.get("ggmlIndex").and_then(Value::as_u64)?;
            (device.get("type").and_then(Value::as_str) == Some(want)).then(|| GpuDevice {
                name: device["name"].as_str().unwrap_or("GPU").to_string(),
                discrete: want == "discrete",
                luid: device
                    .get("luid")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                ggml_index,
                driver_version: device
                    .get("driverVersion")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
            })
        })
    };
    let chosen = pick("discrete").or_else(|| any_gpu.then(|| pick("integrated")).flatten());
    match chosen {
        Some(device) => {
            eprintln!(
                "voicewave: speech worker using GPU '{}' (luid {:?}, ggml {})",
                device.name, device.luid, device.ggml_index
            );
            Placement::Gpu(device)
        }
        None => {
            // Nothing to decode on: don't keep the vk worker around.
            *guard = None;
            eprintln!("voicewave: no usable discrete GPU; speech worker runs on CPU");
            Placement::Cpu
        }
    }
}

fn gpu_device() -> Option<GpuDevice> {
    match &lock(&SESSION).placement {
        Some(Placement::Gpu(device)) => Some(device.clone()),
        _ => None,
    }
}

/// Identity of the driver shader cache a GPU load would hit: worker build,
/// device, driver version. Phase 2's qualification cache replaces this.
fn gpu_cache_key(device: &GpuDevice) -> Option<String> {
    let exe = resolve_worker_exe(Rung::Gpu).ok()?;
    let meta = std::fs::metadata(exe).ok()?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|age| age.as_secs())
        .unwrap_or(0);
    Some(format!(
        "{}:{modified}:{}:{}",
        meta.len(),
        device.luid.as_deref().unwrap_or("-"),
        device.driver_version
    ))
}

fn gpu_ready_marker_path() -> Option<PathBuf> {
    ProjectDirs::from("com", "voicewave", "localcore")
        .map(|dirs| dirs.data_dir().join(GPU_READY_MARKER))
}

/// Whether a GPU load has succeeded before with this worker, device and
/// driver, i.e. the driver's shader cache is warm.
fn gpu_shaders_warm() -> bool {
    let Some(key) = gpu_device().as_ref().and_then(gpu_cache_key) else {
        return false;
    };
    gpu_ready_marker_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .is_some_and(|stored| stored.trim() == key)
}

fn mark_gpu_shaders_warm() {
    let (Some(key), Some(path)) = (
        gpu_device().as_ref().and_then(gpu_cache_key),
        gpu_ready_marker_path(),
    ) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, key);
}

/// Session bookkeeping for a failed GPU operation (plan 019 failure table).
/// Only backend faults count; a first-use load that merely ran out of time
/// (driver still compiling shaders) is retried later. Phase 2 also records
/// backend failures per driver across sessions here.
fn note_gpu_fault(fault: &Fault, first_use_load: bool) {
    let Fault::Worker {
        class,
        reason,
        timed_out,
    } = fault
    else {
        return;
    };
    if *class == ErrorClass::Protocol {
        eprintln!("voicewave: speech worker protocol error (worker bug): {reason}");
    }
    if *class != ErrorClass::Backend || (first_use_load && *timed_out) {
        return;
    }
    let mut session = lock(&SESSION);
    session.gpu_failures += 1;
    if session.gpu_failures == SESSION_GPU_FAILURE_LIMIT {
        eprintln!(
            "voicewave: GPU speech worker failed twice; using CPU for the rest of this session"
        );
    }
}

/// Make sure the slot's worker is alive with `model_path` loaded. Returns
/// (model_init_ms, already_loaded).
fn ensure_loaded(
    guard: &mut Option<WorkerProcess>,
    rung: Rung,
    model_path: &Path,
    deadline: Duration,
) -> Result<(u64, bool), Fault> {
    ensure_spawned(guard, rung)?;
    if guard
        .as_ref()
        .and_then(|worker| worker.loaded.as_ref())
        .is_some_and(|loaded| loaded.model_path == model_path)
    {
        return Ok((0, true));
    }
    let mut body = json!({
        "command": "load",
        "modelPath": model_path.to_string_lossy(),
        "device": "cpu",
        "warmup": true,
    });
    if rung == Rung::Gpu {
        let device =
            gpu_device().ok_or_else(|| Fault::worker(ErrorClass::Backend, "no GPU selected"))?;
        body["device"] = json!("gpu");
        body["ggmlIndex"] = json!(device.ggml_index);
        if let Some(luid) = &device.luid {
            body["luid"] = json!(luid);
        }
        body["flashAttention"] = json!(device.discrete);
    }
    let started = Instant::now();
    with_worker(guard, |worker| {
        worker.loaded = None;
        worker.request(body, deadline, None)?;
        worker.loaded = Some(Loaded {
            model_path: model_path.to_path_buf(),
        });
        Ok(())
    })?;
    if rung == Rung::Gpu {
        mark_gpu_shaders_warm();
    }
    Ok((started.elapsed().as_millis() as u64, false))
}

/// Load the active model into the worker the next dictation will use. No-op
/// when that worker is busy (a load or decode is already running).
fn prewarm_blocking(model_path: &Path) -> Result<(), Fault> {
    let (rung, _) = primary_rung_blocking();
    let Some(mut guard) = lock_slot(rung, Duration::ZERO) else {
        return Ok(());
    };
    let first_use = rung == Rung::Gpu && !gpu_shaders_warm();
    let started = Instant::now();
    let result = ensure_loaded(&mut guard, rung, model_path, load_deadline(rung, first_use));
    match &result {
        Ok((_, false)) => eprintln!(
            "voicewave: {} speech worker ready in {} ms{}",
            rung.dir_name(),
            started.elapsed().as_millis(),
            if first_use { " (first GPU use)" } else { "" }
        ),
        Ok((_, true)) => {}
        Err(fault) if rung == Rung::Gpu => note_gpu_fault(fault, first_use),
        Err(_) => {}
    }
    result.map(|_| ())
}

struct TranscribeJob {
    audio_pcm16_b64: String,
    audio_ms: u64,
    beam_size: u32,
    initial_prompt: Option<String>,
}

fn transcribe_blocking(
    rung: Rung,
    model_path: &Path,
    job: TranscribeJob,
    cancel: &CancellationToken,
) -> Result<FasterWhisperDecodeOutput, Fault> {
    let warm = rung == Rung::Cpu || gpu_shaders_warm();
    // A busy slot means a load is in flight (startup or dictation-start
    // prewarm). Wait for it when it can finish within a normal load; a
    // first-use shader compile can take a minute, so don't wait for that.
    let wait = if warm {
        SPAWN_READY_DEADLINE + load_deadline(rung, false)
    } else {
        Duration::ZERO
    };
    let Some(mut guard) = lock_slot(rung, wait) else {
        return Err(Fault::NotReady(format!(
            "{} speech worker is still loading the model",
            rung.dir_name()
        )));
    };
    if cancel.is_cancelled() {
        return Err(Fault::Cancelled);
    }
    let loaded = guard
        .as_ref()
        .and_then(|worker| worker.loaded.as_ref())
        .is_some_and(|loaded| loaded.model_path == model_path);
    if !warm && !loaded {
        // First GPU use: compile shaders in the background and let this
        // dictation take the CPU rung.
        drop(guard);
        let path = model_path.to_path_buf();
        std::thread::spawn(move || {
            let _ = prewarm_blocking(&path);
        });
        return Err(Fault::NotReady(
            "GPU is getting ready (first use)".to_string(),
        ));
    }

    let result = ensure_loaded(&mut guard, rung, model_path, load_deadline(rung, false)).and_then(
        |(model_init_ms, cache_hit)| {
            let mut body = json!({
                "command": "transcribe",
                "audioPcm16B64": job.audio_pcm16_b64,
                "sampleRateHz": 16_000,
                "beamSize": job.beam_size,
                "language": "en",
            });
            if let Some(prompt) = &job.initial_prompt {
                body["initialPrompt"] = json!(prompt);
            }
            let reply = with_worker(&mut guard, |worker| {
                worker.request(body, transcribe_deadline(rung, job.audio_ms), Some(cancel))
            })?;
            let text = reply["text"]
                .as_str()
                .unwrap_or_default()
                .trim()
                .to_string();
            Ok(FasterWhisperDecodeOutput {
                compression_ratio: compression_ratio(&text),
                text,
                model_init_ms,
                decode_compute_ms: reply["decodeComputeMs"].as_u64().unwrap_or(0),
                runtime_cache_hit: cache_hit,
                segment_count: reply["segmentCount"].as_u64().unwrap_or(0) as u32,
                avg_logprob: reply["avgLogProb"].as_f64().unwrap_or(0.0) as f32,
                no_speech_prob: reply["noSpeechProb"].as_f64().unwrap_or(0.0) as f32,
                backend_requested: rung.backend().to_string(),
                backend_used: reply["backendUsed"].as_str().unwrap_or("cpu").to_string(),
                backend_fallback: false,
            })
        },
    );
    if let Err(fault) = &result {
        if rung == Rung::Gpu {
            note_gpu_fault(fault, false);
        }
    }
    result
}

/// faster-whisper's compression ratio (UTF-8 bytes / zlib bytes), so the
/// shared confidence heuristics see the same signal from both engines.
fn compression_ratio(text: &str) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    let compressed = encoder
        .write_all(text.as_bytes())
        .and_then(|_| encoder.finish())
        .map(|bytes| bytes.len())
        .unwrap_or(0);
    if compressed == 0 {
        0.0
    } else {
        text.len() as f32 / compressed as f32
    }
}

fn join_error(err: tokio::task::JoinError) -> InferenceError {
    InferenceError::RuntimeJoin(format!("speech worker task failed: {err}"))
}

/// Primary rung for the next decode (may spawn the vk worker once per
/// session to list devices).
pub(crate) async fn primary_rung() -> (Rung, Option<String>) {
    tokio::task::spawn_blocking(primary_rung_blocking)
        .await
        .unwrap_or((Rung::Cpu, None))
}

/// One decode on `rung`. `overrides` carries the shared pipeline's beam size
/// and prompt; the worker always decodes at temperature 0 without previous
/// text conditioning.
pub(crate) async fn transcribe(
    rung: Rung,
    model_path: &Path,
    samples: &[f32],
    decode_mode: DecodeMode,
    overrides: &FasterWhisperRequestOverrides,
    cancel: &CancellationToken,
) -> Result<FasterWhisperDecodeOutput, InferenceError> {
    if cancel.is_cancelled() {
        return Err(InferenceError::Cancelled);
    }
    let default_beam = if decode_mode == DecodeMode::Fast {
        1
    } else {
        5
    };
    let job = TranscribeJob {
        audio_pcm16_b64: encode_pcm16_base64(samples),
        audio_ms: samples.len() as u64 * 1000 / 16_000,
        beam_size: overrides.beam_size.unwrap_or(default_beam).clamp(1, 8),
        initial_prompt: overrides
            .initial_prompt
            .as_deref()
            .map(str::trim)
            .filter(|prompt| !prompt.is_empty())
            .map(str::to_string),
    };
    let model_path = model_path.to_path_buf();
    let cancel = cancel.clone();
    tokio::task::spawn_blocking(move || transcribe_blocking(rung, &model_path, job, &cancel))
        .await
        .map_err(join_error)?
        .map_err(InferenceError::from)
}

/// Spawn and load the worker the next dictation will use, so it starts warm.
/// Serialized by the caller against polish prewarm; returns once loaded.
pub async fn prewarm_asr_worker(model_path: PathBuf) -> Result<(), InferenceError> {
    tokio::task::spawn_blocking(move || prewarm_blocking(&model_path))
        .await
        .map_err(join_error)?
        .map_err(InferenceError::from)
}

/// Drop workers that are idle (e.g. after switching to a faster-whisper
/// model), freeing the VRAM/RAM their model holds.
pub fn release_idle_asr_workers() {
    for rung in [Rung::Gpu, Rung::Cpu] {
        if let Ok(mut guard) = slot(rung).try_lock() {
            *guard = None;
        }
    }
}

/// Kill every speech worker. Called on app exit.
pub fn kill_asr_workers() {
    release_idle_asr_workers();
    let pids = std::mem::take(&mut *lock(&LIVE_PIDS));
    for pid in pids {
        kill_process_by_pid(pid);
    }
}
