//! whisper.cpp speech workers (plan 019): process transport, deadlines, the
//! GPU/CPU rung bookkeeping behind `InferenceBackend::AsrWorker`, and the
//! GPU qualification (the worker's first start with the user's real model).
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
    encode_pcm16_base64, kill_process_by_pid, transcribe_samples_with_overrides,
    FasterWhisperDecodeOutput, FasterWhisperRequestOverrides,
};
use super::gpu_select::{
    self, cpu_support, hardware_profile, CpuSupport, GpuDevice, QualOutcome, QualificationRecord,
};
use super::InferenceError;
use crate::settings::DecodeMode;
use flate2::{write::ZlibEncoder, Compression};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::fmt;
use std::io::{BufRead, BufReader, Read, Write};
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc::{self, RecvTimeoutError},
    Arc, Mutex, MutexGuard, OnceLock, TryLockError,
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio_util::sync::CancellationToken;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

// Per-phase deadlines (plan 019 "Deadlines").
const SPAWN_READY_DEADLINE: Duration = Duration::from_secs(10);
const LIST_DEVICES_DEADLINE: Duration = Duration::from_secs(5);
const LOAD_GPU_DEADLINE: Duration = Duration::from_secs(30);
/// GPU loads off the dictation path (qualification, prewarm). The first load
/// on a machine, and after a driver or worker update, waits for the driver to
/// compile ggml's shaders: 25-41 s measured on an RTX 3060. Missing it is
/// "retry later", never a GPU failure.
const LOAD_GPU_FIRST_USE_DEADLINE: Duration = Duration::from_secs(120);
/// Generous for slow laptops loading Large Turbo from a cold disk.
const LOAD_CPU_DEADLINE: Duration = Duration::from_secs(60);
/// Upper bound on waiting for a killed worker to actually exit.
const REAP_WAIT: Duration = Duration::from_secs(5);
/// How often a waiting request checks the user's cancel token.
const POLL_SLICE: Duration = Duration::from_millis(50);
const STDERR_TAIL_LINES: usize = 40;
/// Longest failure reason kept for logs and diagnostics.
const MAX_REASON_CHARS: usize = 320;
/// Backend failures in a row after which dictation uses the CPU for
/// `GPU_COOLDOWN`, then gives the GPU another chance.
const SESSION_GPU_FAILURE_LIMIT: u32 = 2;
const GPU_COOLDOWN: Duration = Duration::from_secs(5 * 60);
/// Load deadline for the fresh GPU worker a dictation retries on after a GPU
/// fault: a warm (shader-cached) load takes 2-8 s, and a driver that is still
/// stuck should cost the user little before the CPU rung takes over.
pub(crate) const GPU_RETRY_LOAD_DEADLINE: Duration = Duration::from_secs(12);
/// Dictation decodes after a GPU model load that get the cold deadline (see
/// `transcribe_deadline`). On a fresh driver shader cache the first decode of
/// each new audio length compiles shaders; after a handful every length runs
/// in 0.3-0.5 s on an RTX 3060.
const GPU_COLD_DECODES: u32 = 5;

// Qualification (plan 019): pass bars for a discrete GPU, warm decodes.
const QUALIFY_SHORT_MAX_MS: u64 = 1_000;
const QUALIFY_LONG_MAX_MS: u64 = 1_500;
/// Word error rate above which a clip's transcript counts as wrong. A broken
/// GPU path produces empty, repeated or garbage text, far past this.
const QUALIFY_MAX_WER: f32 = 0.2;
const QUALIFY_BEAM: u32 = 5;
/// Bundled clips (LibriSpeech test-clean, CC BY 4.0; see
/// windows/asr-worker/qualify/ATTRIBUTION.txt) and their reference text.
const QUALIFY_CLIPS: [(&str, &str); 2] = [
    ("short.wav", "now let's dust the furniture and pictures"),
    (
        "long.wav",
        "the night was clear starlit and splendid the tempest had passed away and the sweet \
         influences of the evening had restored life peace and security everywhere",
    ),
];
/// Bump when the clips, bars or decode settings above change: part of the
/// qualification cache key.
const QUALIFY_CONFIG_VERSION: u32 = 1;
/// `Retry` results in a row for one key before it is recorded as `Failed`,
/// so a borderline GPU stops rerunning the full check at every launch.
const QUALIFY_MAX_RETRIES: usize = 3;
/// Each qualification decode. The clips run on a just-loaded worker, so they
/// are cold decodes (a 10.8 s decode took 7.7 s on a fresh shader cache);
/// a timeout here caches a GPU fail, so this matches the cold dictation
/// deadline for the 10.8 s clip (~26 s), rounded up to one constant.
const QUALIFY_DECODE_DEADLINE: Duration = Duration::from_secs(30);
/// Longest a dictation waits for the one-time GPU check when the only other
/// choice is the CPU worker (see `wait_for_gpu_check`): the check's own worst
/// case, i.e. spawn, first-use load, two tries at each clip, and a reap.
/// It usually finishes in 25-50 s.
pub(crate) const GPU_CHECK_WAIT: Duration = Duration::from_secs(
    SPAWN_READY_DEADLINE.as_secs()
        + LOAD_GPU_FIRST_USE_DEADLINE.as_secs()
        + 2 * QUALIFY_CLIPS.len() as u64 * QUALIFY_DECODE_DEADLINE.as_secs()
        + REAP_WAIT.as_secs(),
);
/// How often that wait checks whether the GPU check is done.
const GPU_CHECK_POLL: Duration = Duration::from_millis(250);

/// `cold` = one of the first `GPU_COLD_DECODES` decodes since the GPU
/// worker's model load (ignored on the CPU).
fn transcribe_deadline(rung: Rung, audio_ms: u64, cold: bool) -> Duration {
    match rung {
        // The driver may compile shaders for this audio length first: a 5 s
        // dictation took over 6 s, a 10.8 s one 7.7 s, on an RTX 3060 with a
        // fresh shader cache. 15 s + 1x audio leaves ~3x headroom for slower
        // GPUs; a miss here is a strike toward the GPU cooldown, so being
        // generous for a few decodes is cheaper than a false fault.
        Rung::Gpu if cold => Duration::from_millis(15_000 + audio_ms),
        // Warm decodes run ~0.07x real time (0.25x right after the GPU
        // idles down), so this catches a hung GPU in seconds, not minutes.
        Rung::Gpu => Duration::from_millis(4_000 + audio_ms * 2 / 5),
        Rung::Cpu => Duration::from_millis(30_000 + audio_ms.saturating_mul(3)),
    }
}

fn load_deadline(rung: Rung) -> Duration {
    match rung {
        Rung::Gpu => LOAD_GPU_DEADLINE,
        Rung::Cpu => LOAD_CPU_DEADLINE,
    }
}

/// Worst case for one decode on `rung`: spawn (or waiting for a load already
/// in flight), load, primary + confidence-retry decodes (both cold, the worst
/// case), reaps. Only a backstop for the controller's outer timeout; the
/// per-phase deadlines above always fire first.
pub(crate) fn rung_budget(rung: Rung, audio_ms: u64) -> Duration {
    let setup = match rung {
        Rung::Gpu => LIST_DEVICES_DEADLINE + SPAWN_READY_DEADLINE + LOAD_GPU_DEADLINE,
        Rung::Cpu => SPAWN_READY_DEADLINE + LOAD_CPU_DEADLINE,
    };
    setup + transcribe_deadline(rung, audio_ms, true) * 2 + REAP_WAIT * 2
}

/// Worst case for the fresh GPU worker retried after a GPU fault. Its decodes
/// are cold: a new process compiles its own pipelines.
pub(crate) fn gpu_retry_budget(audio_ms: u64) -> Duration {
    SPAWN_READY_DEADLINE
        + GPU_RETRY_LOAD_DEADLINE
        + transcribe_deadline(Rung::Gpu, audio_ms, true) * 2
        + REAP_WAIT * 2
}

/// Which worker a decode runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
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

/// A GGML model as the workers see it. `sha256` (pinned and verified at
/// download) is part of the qualification cache key.
#[derive(Debug, Clone)]
pub struct AsrModel {
    pub id: String,
    pub path: PathBuf,
    pub sha256: String,
}

impl AsrModel {
    /// NVIDIA Parakeet (the worker's "parakeet" engine) rather than whisper.
    pub fn is_parakeet(&self) -> bool {
        super::is_parakeet_model(&self.id)
    }
}

/// Why a dictation did not decode on the GPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepDownKind {
    /// The one-time GPU check (driver shader compile + test clips) is running.
    Preparing,
    /// GPU off for now: failed or slow qualification, per-driver disable,
    /// repeated failures this session.
    Unavailable,
    /// The GPU attempt of this dictation failed; the CPU rung retried it.
    Fault,
}

#[derive(Debug, Clone)]
pub struct StepDown {
    pub kind: StepDownKind,
    pub reason: String,
}

impl StepDown {
    fn new(kind: StepDownKind, reason: impl AsRef<str>) -> Self {
        Self {
            kind,
            reason: trim_reason(reason.as_ref()),
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
    /// The worker is busy loading (prewarm or qualification in flight). The
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
            Fault::Worker { class, reason, .. } => InferenceError::AsrWorker {
                class,
                reason: trim_reason(&reason),
            },
        }
    }
}

/// Keep failure text (which can carry a worker's stderr) log-sized, with the
/// user's profile folder (it carries their name, e.g. in a model path)
/// replaced: this text lands in gpu-engine.json and the diagnostics export.
pub(crate) fn trim_reason(reason: &str) -> String {
    let reason = redact_profile_dir(reason.trim());
    if reason.chars().count() <= MAX_REASON_CHARS {
        return reason;
    }
    let mut trimmed: String = reason.chars().take(MAX_REASON_CHARS - 1).collect();
    trimmed.push('…');
    trimmed
}

/// `C:\Users\<name>` → `%USERPROFILE%`, ignoring ASCII case like Windows.
fn redact_profile_dir(text: &str) -> String {
    let needle = std::env::var("USERPROFILE")
        .unwrap_or_default()
        .trim_end_matches(['\\', '/'])
        .to_ascii_lowercase();
    if needle.len() <= 3 {
        return text.to_string();
    }
    // ASCII lowercasing keeps byte offsets, so they index `text` too.
    let haystack = text.to_ascii_lowercase();
    let mut redacted = String::with_capacity(text.len());
    let mut at = 0;
    while let Some(found) = haystack[at..].find(&needle) {
        redacted.push_str(&text[at..at + found]);
        redacted.push_str("%USERPROFILE%");
        at += found + needle.len();
    }
    redacted.push_str(&text[at..]);
    redacted
}

type StderrTail = Arc<Mutex<VecDeque<String>>>;

struct Loaded {
    model_path: PathBuf,
    /// Dictation decodes since this load (picks the cold GPU deadline).
    decodes: u32,
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
        kill_and_reap(&mut self.child);
        // Forget the pid even if the exit is unconfirmed: the termination is
        // already queued (a worker wedged in a driver call dies when the call
        // returns), and a remembered pid could be reused by another process.
        if let Ok(mut pids) = LIVE_PIDS.lock() {
            pids.retain(|value| *value != pid);
        }
    }
}

/// Kill a worker and wait up to `REAP_WAIT` for it to exit. Never blocks
/// longer: a process wedged in a driver call dies when the call returns.
fn kill_and_reap(child: &mut Child) {
    let pid = child.id();
    let _ = child.kill();
    let deadline = Instant::now() + REAP_WAIT;
    loop {
        match child.try_wait() {
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                tracing::warn!(
                    "asr worker pid {pid} still running {} ms after kill",
                    REAP_WAIT.as_millis()
                );
                return;
            }
            _ => return,
        }
    }
}

impl WorkerProcess {
    fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// The last few stderr lines, each cut short: enough to classify a crash
    /// without dragging a log into the step-down reason.
    fn stderr_text(&self) -> String {
        let Ok(tail) = self.stderr_tail.lock() else {
            return String::new();
        };
        let skip = tail.len().saturating_sub(3);
        tail.iter()
            .skip(skip)
            .map(|line| line.chars().take(120).collect::<String>())
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

    fn wait_ready(&mut self, deadline: Duration) -> Result<(), Fault> {
        let until = Instant::now() + deadline;
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
            // Replies carry transcript text, and fault reasons reach the log,
            // gpu-engine.json and the diagnostics export: report sizes only.
            let reply: Value = serde_json::from_str(line.trim()).map_err(|_| {
                Fault::worker(
                    ErrorClass::Protocol,
                    format!("unparseable reply ({} bytes)", line.len()),
                )
            })?;
            let reply_id = reply.get("id").and_then(Value::as_u64);
            if reply_id.is_some() && reply_id == cancel_id {
                continue; // the cancel acknowledgement
            }
            if reply_id != Some(id) {
                return Err(Fault::worker(
                    ErrorClass::Protocol,
                    format!("reply id mismatch (expected {id}, got {reply_id:?})"),
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

/// What the worker reported running the last load or decode.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunningOn {
    pub device: Rung,
    /// GPU name for `Gpu`; `None` for the processor.
    pub name: Option<String>,
}

static GPU_SLOT: Mutex<Option<WorkerProcess>> = Mutex::new(None);
static CPU_SLOT: Mutex<Option<WorkerProcess>> = Mutex::new(None);
/// Pids of every live worker, so app exit can reach a worker whose slot is
/// held by an in-flight request.
static LIVE_PIDS: Mutex<Vec<u32>> = Mutex::new(Vec::new());
/// Settings → Dictation → GPU acceleration (Auto = true, Off = false).
static GPU_ENABLED: AtomicBool = AtomicBool::new(true);
static SESSION: Mutex<Session> = Mutex::new(Session {
    gpu_failures: 0,
    gpu_cooldown_until: None,
    qualifying: None,
    retry_next_session: Vec::new(),
    last_report: None,
});

struct Session {
    /// Backend failures of the GPU rung since its last success or cooldown.
    gpu_failures: u32,
    /// After repeated failures: CPU until then, then the GPU again.
    gpu_cooldown_until: Option<Instant>,
    /// Cache key of the qualification running now.
    qualifying: Option<String>,
    /// Keys whose qualification was correct-but-slow or hit a transient
    /// error: CPU for the rest of this session, retried on the next launch.
    retry_next_session: Vec<(String, String)>,
    /// Model path + what the worker reported for its last load or decode.
    last_report: Option<(PathBuf, RunningOn)>,
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

fn ensure_spawned(
    guard: &mut Option<WorkerProcess>,
    rung: Rung,
    ready_deadline: Duration,
) -> Result<(), Fault> {
    if let Some(worker) = guard.as_mut() {
        if worker.alive() {
            return Ok(());
        }
        // Found dead while idle (sleep/resume, driver reset): respawn
        // lazily, not a failure.
        tracing::warn!(
            "{} speech worker found dead while idle; respawning",
            rung.dir_name()
        );
        *guard = None;
    }
    *guard = Some(spawn(rung, ready_deadline)?);
    Ok(())
}

fn worker_command(exe: &Path) -> Command {
    let mut command = Command::new(exe);
    #[cfg(target_os = "windows")]
    command.creation_flags(CREATE_NO_WINDOW);
    // Implicit Vulkan layers (RTSS, OBS, Steam and Epic overlays) hook every
    // Vulkan app and can crash ggml-vulkan. Loaders from 1.3.234 skip them
    // all with this filter; older loaders ignore the variable harmlessly.
    command.env("VK_LOADER_LAYERS_DISABLE", "~implicit~");
    command
}

fn spawn(rung: Rung, ready_deadline: Duration) -> Result<WorkerProcess, Fault> {
    // Both workers are built for AVX2 + FMA + F16C; never start one on a CPU
    // it would crash on.
    if cpu_support() != CpuSupport::Full {
        return Err(Fault::worker(
            ErrorClass::Backend,
            "this processor lacks AVX2, which this speech model needs; choose a model in Models",
        ));
    }
    let exe = resolve_worker_exe(rung)?;
    let mut command = worker_command(&exe);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|err| {
        Fault::worker(
            ErrorClass::Backend,
            format!("failed to start {}: {err}", exe.display()),
        )
    })?;
    crate::job_object::assign(&child);
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
    worker.wait_ready(ready_deadline)?;
    Ok(worker)
}

/// A worker resource (`asr-worker/<relative>`): the bundled copy, or the dev
/// tree (`windows/asr-worker/<dev_relative>`).
fn resolve_resource(relative: &Path, dev_relative: &Path) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
    {
        candidates.push(exe_dir.join("asr-worker").join(relative));
        candidates.push(exe_dir.join("resources").join("asr-worker").join(relative));
    }
    candidates.push(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("windows")
            .join("asr-worker")
            .join(dev_relative),
    );
    candidates.into_iter().find(|path| path.exists())
}

fn resolve_worker_exe(rung: Rung) -> Result<PathBuf, Fault> {
    let relative = Path::new(rung.dir_name()).join(rung.exe_name());
    // Dev builds: the output of scripts/asr-worker/build-asr-worker.ps1.
    resolve_resource(&relative, &Path::new("bin").join(&relative)).ok_or_else(|| {
        Fault::worker(
            ErrorClass::Backend,
            format!("{} is missing from this install", rung.exe_name()),
        )
    })
}

/// `voicewave-asr-vk.exe --list-devices`: the GPUs Vulkan reports, without a
/// model (~0.3 s). Killed and reaped after the 5 s deadline.
pub(crate) fn list_devices() -> Result<Vec<Value>, String> {
    let exe = resolve_worker_exe(Rung::Gpu).map_err(|fault| InferenceError::from(fault).to_string())?;
    let mut child = worker_command(&exe)
        .arg("--list-devices")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| format!("failed to start {}: {err}", exe.display()))?;
    crate::job_object::assign(&child);
    let mut stdout = child.stdout.take().ok_or("list-devices stdout unavailable")?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.read_to_string(&mut text);
        let _ = tx.send(text);
    });
    let output = rx.recv_timeout(LIST_DEVICES_DEADLINE);
    // Bounded: a driver hung in vkCreateInstance must not hold the hardware
    // profile lock (and with it the model list) forever.
    kill_and_reap(&mut child);
    let text = output.map_err(|_| "list-devices missed its 5 s deadline".to_string())?;
    let reply: Value = serde_json::from_str(text.trim())
        .map_err(|_| format!("unparseable device list: {}", trim_reason(&text)))?;
    if reply.get("ok") != Some(&Value::Bool(true)) {
        return Err(format!("device list failed: {}", trim_reason(&text)));
    }
    Ok(reply["devices"].as_array().cloned().unwrap_or_default())
}

/// Worker build fingerprint for the qualification cache and the per-driver
/// disable: a hash of the Vulkan worker EXE (an app update that ships the
/// same worker keeps its cached results). Hashed once per session.
pub(crate) fn worker_fingerprint() -> String {
    static FINGERPRINT: OnceLock<String> = OnceLock::new();
    FINGERPRINT
        .get_or_init(|| {
            let Ok(exe) = resolve_worker_exe(Rung::Gpu) else {
                return "missing".to_string();
            };
            let Ok(mut file) = std::fs::File::open(&exe) else {
                return "unreadable".to_string();
            };
            let mut hasher = Sha256::new();
            let mut buffer = vec![0_u8; 1 << 20];
            loop {
                match file.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(read) => hasher.update(&buffer[..read]),
                    Err(_) => return "unreadable".to_string(),
                }
            }
            hasher
                .finalize()
                .iter()
                .take(8)
                .map(|byte| format!("{byte:02x}"))
                .collect()
        })
        .clone()
}

/// Settings → Dictation → GPU acceleration. Off drops the GPU worker (and
/// the VRAM its model holds) when it is idle.
pub fn set_gpu_acceleration(enabled: bool) {
    let was_enabled = GPU_ENABLED.swap(enabled, Ordering::Relaxed);
    if was_enabled && !enabled {
        if let Ok(mut guard) = GPU_SLOT.try_lock() {
            *guard = None;
        }
    }
}

/// `gpu_select::gpu_ruled_out` matches on this layout's device, driver and
/// worker parts; keep them in sync.
fn qualification_key(device: &GpuDevice, model: &AsrModel) -> String {
    format!(
        "{}|driver {}|{} {}|worker {}|q{QUALIFY_CONFIG_VERSION} beam{QUALIFY_BEAM} fa{}",
        device.identity(),
        device.driver_version,
        model.id,
        model.sha256,
        worker_fingerprint(),
        u8::from(device.discrete)
    )
}

enum GpuReadiness {
    Ready,
    /// Not qualified yet (or qualifying now).
    Preparing,
    Unavailable(String),
}

fn gpu_readiness(key: &str) -> GpuReadiness {
    {
        let session = lock(&SESSION);
        if session.qualifying.as_deref() == Some(key) {
            return GpuReadiness::Preparing;
        }
        if let Some((_, reason)) = session
            .retry_next_session
            .iter()
            .find(|(retry_key, _)| retry_key == key)
        {
            return GpuReadiness::Unavailable(format!("gpu-retry-next-session: {reason}"));
        }
    }
    match gpu_select::cached_qualification(key) {
        Some(QualOutcome::Passed) => GpuReadiness::Ready,
        Some(_) => GpuReadiness::Unavailable(
            "gpu-qualification-failed: this graphics card failed the one-time check".to_string(),
        ),
        None => GpuReadiness::Preparing,
    }
}

/// Where the next dictation decodes first, plus why it is not the GPU when
/// it is not and the user didn't choose that (GPU acceleration Off and
/// machines without a qualifying GPU carry no step-down).
fn primary_rung_blocking(model: &AsrModel) -> (Rung, Option<StepDown>) {
    if !GPU_ENABLED.load(Ordering::Relaxed) {
        // A GPU worker that was busy when GPU acceleration went Off.
        release_idle_rung(Rung::Gpu);
        return (Rung::Cpu, None);
    }
    let Some(device) = hardware_profile().gpu else {
        return (Rung::Cpu, None);
    };
    {
        let mut session = lock(&SESSION);
        match session.gpu_cooldown_until {
            Some(until) if Instant::now() < until => {
                return (
                    Rung::Cpu,
                    Some(StepDown::new(
                        StepDownKind::Unavailable,
                        "gpu-cooldown: repeated GPU failures, retrying the GPU in a few minutes",
                    )),
                );
            }
            Some(_) => session.gpu_cooldown_until = None,
            None => {}
        }
    }
    if gpu_select::driver_disabled(&device) {
        return (
            Rung::Cpu,
            Some(StepDown::new(
                StepDownKind::Unavailable,
                "driver-disabled: GPU failures in two sessions on this driver",
            )),
        );
    }
    match gpu_readiness(&qualification_key(&device, model)) {
        GpuReadiness::Ready => (Rung::Gpu, None),
        GpuReadiness::Preparing => (
            Rung::Cpu,
            Some(StepDown::new(
                StepDownKind::Preparing,
                "gpu-preparing: one-time graphics card check",
            )),
        ),
        GpuReadiness::Unavailable(reason) => {
            (Rung::Cpu, Some(StepDown::new(StepDownKind::Unavailable, reason)))
        }
    }
}

/// Session bookkeeping for a failed GPU operation (plan 019 failure table).
/// Only backend faults count; a load off the dictation path that merely ran
/// out of time (driver compiling shaders) is retried later. Counted faults
/// also feed the per-driver disable.
fn note_gpu_fault(fault: &Fault, background_load: bool) {
    let Fault::Worker {
        class,
        reason,
        timed_out,
    } = fault
    else {
        return;
    };
    if *class == ErrorClass::Protocol {
        tracing::error!(
            "speech worker protocol error (worker bug): {}",
            trim_reason(reason)
        );
    }
    if *class != ErrorClass::Backend || (background_load && *timed_out) {
        return;
    }
    {
        let mut session = lock(&SESSION);
        session.gpu_failures += 1;
        if session.gpu_failures < SESSION_GPU_FAILURE_LIMIT {
            return;
        }
        session.gpu_failures = 0;
        session.gpu_cooldown_until = Some(Instant::now() + GPU_COOLDOWN);
        tracing::warn!(
            "GPU speech worker failed twice in a row; using CPU for {} min",
            GPU_COOLDOWN.as_secs() / 60
        );
    }
    // Only a cooldown counts toward the per-driver disable: one stray fault
    // per session must never turn the GPU off for a week.
    if let Some(device) = hardware_profile().gpu {
        if gpu_select::record_backend_failure(&device) {
            tracing::warn!(
                "GPU failed in two sessions on driver {}; disabled for 7 days or until an update",
                device.driver_version
            );
        }
    }
}

fn note_report(model_path: &Path, reply: &Value) {
    let device = if reply["backendUsed"].as_str() == Some("vulkan") {
        Rung::Gpu
    } else {
        Rung::Cpu
    };
    let name = (device == Rung::Gpu)
        .then(|| reply["deviceName"].as_str().map(str::to_string))
        .flatten();
    lock(&SESSION).last_report = Some((model_path.to_path_buf(), RunningOn { device, name }));
}

/// Make sure the slot's worker is alive with `model` loaded. Returns
/// (model_init_ms, already_loaded).
fn ensure_loaded(
    guard: &mut Option<WorkerProcess>,
    rung: Rung,
    model: &AsrModel,
    deadline: Duration,
) -> Result<(u64, bool), Fault> {
    let model_path = model.path.as_path();
    // A short load deadline (the fresh-worker retry) shortens the start
    // too: a worker that hangs while starting must not cost the full 10 s.
    ensure_spawned(guard, rung, SPAWN_READY_DEADLINE.min(deadline / 3))?;
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
    if model.is_parakeet() {
        body["engine"] = json!("parakeet");
    }
    if rung == Rung::Gpu {
        let device = hardware_profile()
            .gpu
            .ok_or_else(|| Fault::worker(ErrorClass::Backend, "no GPU selected"))?;
        body["device"] = json!("gpu");
        body["ggmlIndex"] = json!(device.ggml_index);
        if let Some(luid) = &device.luid {
            body["luid"] = json!(luid);
        }
        // Flash attention on for discrete GPUs, off for integrated.
        body["flashAttention"] = json!(device.discrete);
    } else {
        // The CPU does all the work here: every hardware thread but two,
        // left for the app and the rest of the PC.
        let threads = std::thread::available_parallelism().map_or(4, |count| count.get());
        body["threads"] = json!(threads.saturating_sub(2).max(2));
    }
    let started = Instant::now();
    let reply = with_worker(guard, |worker| {
        worker.loaded = None;
        let reply = worker.request(body, deadline, None)?;
        worker.loaded = Some(Loaded {
            model_path: model_path.to_path_buf(),
            decodes: 0,
        });
        Ok(reply)
    })?;
    note_report(model_path, &reply);
    Ok((started.elapsed().as_millis() as u64, false))
}

/// What a prewarm found to do.
enum Prewarm {
    Done,
    /// The GPU path is not qualified for this model yet: run qualification.
    Qualify,
}

/// Load the model into the worker the next dictation will use. No-op when
/// that worker is busy (a load or decode is already running).
fn prewarm_blocking(model: &AsrModel) -> Result<Prewarm, Fault> {
    let (rung, step_down) = primary_rung_blocking(model);
    if step_down.is_some_and(|step| step.kind == StepDownKind::Preparing) {
        return Ok(Prewarm::Qualify);
    }
    let Some(mut guard) = lock_slot(rung, Duration::ZERO) else {
        return Ok(Prewarm::Done);
    };
    let deadline = match rung {
        // Off the dictation path, and the driver's shader cache may have
        // been reset since qualification.
        Rung::Gpu => LOAD_GPU_FIRST_USE_DEADLINE,
        Rung::Cpu => LOAD_CPU_DEADLINE,
    };
    let started = Instant::now();
    let result = ensure_loaded(&mut guard, rung, model, deadline);
    match &result {
        Ok((_, false)) => tracing::info!(
            "{} speech worker ready in {} ms",
            rung.dir_name(),
            started.elapsed().as_millis()
        ),
        Ok((_, true)) => {}
        Err(fault) if rung == Rung::Gpu => note_gpu_fault(fault, true),
        Err(_) => {}
    }
    result.map(|_| Prewarm::Done)
}

struct TranscribeJob {
    load_deadline: Duration,
    audio_pcm16_b64: String,
    audio_ms: u64,
    beam_size: u32,
    initial_prompt: Option<String>,
    /// Parakeet only: dictionary terms its decoder boosts (plan 021).
    boost_terms: Vec<String>,
}

fn transcribe_blocking(
    rung: Rung,
    model: &AsrModel,
    job: TranscribeJob,
    cancel: &CancellationToken,
) -> Result<FasterWhisperDecodeOutput, Fault> {
    let model_path = model.path.as_path();
    // A busy slot means a load is in flight (startup or dictation-start
    // prewarm). Wait for it when it can finish within a normal load.
    let Some(mut guard) = lock_slot(rung, SPAWN_READY_DEADLINE + load_deadline(rung)) else {
        return Err(Fault::NotReady(format!(
            "{} speech worker is still loading the model",
            rung.dir_name()
        )));
    };
    if cancel.is_cancelled() {
        return Err(Fault::Cancelled);
    }

    let result = ensure_loaded(&mut guard, rung, model, job.load_deadline).and_then(
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
            if !job.boost_terms.is_empty() {
                body["boostTerms"] = json!(job.boost_terms);
            }
            let reply = with_worker(&mut guard, |worker| {
                // Only dictation decodes count: the qualification's two clip
                // lengths leave the user's own lengths still to compile.
                let decode_number = worker.loaded.as_mut().map_or(1, |loaded| {
                    loaded.decodes += 1;
                    loaded.decodes
                });
                let cold = decode_number <= GPU_COLD_DECODES;
                worker.request(body, transcribe_deadline(rung, job.audio_ms, cold), Some(cancel))
            })?;
            note_report(model_path, &reply);
            if rung == Rung::Gpu {
                lock(&SESSION).gpu_failures = 0;
            }
            // Counts only: never a term or transcript text in the log.
            if let Some(boost) = reply.get("boost") {
                tracing::debug!(
                    "parakeet boost: {} terms, {} skipped, {} matched, {} changed, {} ms",
                    boost["terms"],
                    boost["skipped"],
                    boost["completed"],
                    boost["flipped"],
                    boost["ms"]
                );
            }
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

// --------------------------------------------------------- qualification ---

pub(crate) struct QualifyClip {
    name: &'static str,
    reference: String,
    samples: Vec<f32>,
}

pub(crate) fn load_qualify_clips() -> Result<Vec<QualifyClip>, String> {
    QUALIFY_CLIPS
        .iter()
        .map(|(name, reference)| {
            let relative = Path::new("qualify").join(name);
            let path = resolve_resource(&relative, &relative)
                .ok_or_else(|| format!("qualification clip {name} is missing from this install"))?;
            Ok(QualifyClip {
                name,
                reference: (*reference).to_string(),
                samples: read_wav_16k_mono(&path)?,
            })
        })
        .collect()
}

/// 16 kHz mono 16-bit PCM WAV → samples in [-1, 1].
fn read_wav_16k_mono(path: &Path) -> Result<Vec<f32>, String> {
    let bytes = std::fs::read(path).map_err(|err| format!("{}: {err}", path.display()))?;
    let bad = || format!("{} is not 16 kHz mono 16-bit PCM", path.display());
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(bad());
    }
    let u16_at = |at: usize| u16::from_le_bytes([bytes[at], bytes[at + 1]]);
    let u32_at = |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    let mut at = 12;
    let mut format_ok = false;
    while at + 8 <= bytes.len() {
        let size = u32_at(at + 4) as usize;
        let body = at + 8;
        let end = body.saturating_add(size).min(bytes.len());
        match &bytes[at..at + 4] {
            b"fmt " if size >= 16 => {
                format_ok = u16_at(body) == 1
                    && u16_at(body + 2) == 1
                    && u32_at(body + 4) == 16_000
                    && u16_at(body + 14) == 16;
            }
            b"data" if format_ok => {
                return Ok(bytes[body..end]
                    .chunks_exact(2)
                    .map(|pair| i16::from_le_bytes([pair[0], pair[1]]) as f32 / 32768.0)
                    .collect());
            }
            _ => {}
        }
        at = body + size + (size & 1);
    }
    Err(bad())
}

fn normalize_words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split_whitespace()
        .map(|word| {
            word.chars()
                .filter(|c| c.is_alphanumeric() || *c == '\'')
                .collect::<String>()
        })
        .filter(|word| !word.is_empty())
        .collect()
}

/// Word error rate of `hypothesis` against `reference` (word-level edit
/// distance / reference length), after lowercasing and dropping punctuation.
fn word_error_rate(reference: &str, hypothesis: &str) -> f32 {
    let reference = normalize_words(reference);
    let hypothesis = normalize_words(hypothesis);
    if reference.is_empty() {
        return if hypothesis.is_empty() { 0.0 } else { 1.0 };
    }
    let mut previous: Vec<usize> = (0..=hypothesis.len()).collect();
    for (i, ref_word) in reference.iter().enumerate() {
        let mut current = vec![i + 1; hypothesis.len() + 1];
        for (j, hyp_word) in hypothesis.iter().enumerate() {
            let substitution = previous[j] + usize::from(ref_word != hyp_word);
            current[j + 1] = substitution.min(previous[j + 1] + 1).min(current[j] + 1);
        }
        previous = current;
    }
    previous[hypothesis.len()] as f32 / reference.len() as f32
}

enum Verdict {
    Passed,
    Failed(String),
    Retry(String),
    /// The check could not run fully (GPU memory busy, interrupted): retry
    /// next session, never counted toward `QUALIFY_MAX_RETRIES`.
    Inconclusive(String),
}

#[derive(Default)]
struct QualifyTimings {
    load_ms: u64,
    short_ms: Option<u64>,
    long_ms: Option<u64>,
}

/// Map a worker fault during qualification to its outcome (plan 019
/// outcomes table).
fn fault_verdict(fault: Fault, during_load: bool) -> Verdict {
    match fault {
        Fault::Worker {
            class: ErrorClass::Backend,
            timed_out: true,
            reason,
        } if during_load => Verdict::Retry(format!("load ran past 120 s (driver still preparing): {reason}")),
        Fault::Worker {
            class: ErrorClass::Backend,
            reason,
            ..
        } => Verdict::Failed(reason),
        Fault::Worker {
            class: ErrorClass::Resource,
            reason,
            ..
        } => Verdict::Inconclusive(reason),
        Fault::Worker { class, reason, .. } => Verdict::Retry(format!("{class}: {reason}")),
        Fault::Cancelled | Fault::NotReady(_) => Verdict::Inconclusive("interrupted".to_string()),
    }
}

/// Load `model` on the GPU (warm-up included), then decode each clip and
/// hold the result to the pass rules. Leaves the worker loaded.
fn qualify_gpu_blocking(model: &AsrModel, clips: &[QualifyClip]) -> (Verdict, QualifyTimings) {
    let mut timings = QualifyTimings::default();
    let mut guard = lock(&GPU_SLOT);
    // Start clean so the load (and any shader compile) is measured honestly.
    *guard = None;
    let started = Instant::now();
    if let Err(fault) = ensure_loaded(&mut guard, Rung::Gpu, model, LOAD_GPU_FIRST_USE_DEADLINE) {
        return (fault_verdict(fault, true), timings);
    }
    timings.load_ms = started.elapsed().as_millis() as u64;
    let never_cancel = CancellationToken::new();
    for (index, clip) in clips.iter().enumerate() {
        let limit = if index == 0 {
            QUALIFY_SHORT_MAX_MS
        } else {
            QUALIFY_LONG_MAX_MS
        };
        let mut best_ms = u64::MAX;
        // A second try for a slow first decode (GPU clocking up).
        for _attempt in 0..2 {
            let body = json!({
                "command": "transcribe",
                "audioPcm16B64": encode_pcm16_base64(&clip.samples),
                "sampleRateHz": 16_000,
                "beamSize": QUALIFY_BEAM,
                "language": "en",
            });
            let reply = match with_worker(&mut guard, |worker| {
                worker.request(body, QUALIFY_DECODE_DEADLINE, Some(&never_cancel))
            }) {
                Ok(reply) => reply,
                Err(fault) => return (fault_verdict(fault, false), timings),
            };
            let backend = reply["backendUsed"].as_str().unwrap_or("unknown");
            if backend != "vulkan" {
                return (
                    Verdict::Failed(format!("GPU not actually used ({backend} ran {})", clip.name)),
                    timings,
                );
            }
            let text = reply["text"].as_str().unwrap_or_default();
            let wer = word_error_rate(&clip.reference, text);
            if wer > QUALIFY_MAX_WER {
                return (
                    Verdict::Failed(format!(
                        "wrong transcript for {} (WER {:.0}%): \"{}\"",
                        clip.name,
                        wer * 100.0,
                        text.chars().take(80).collect::<String>()
                    )),
                    timings,
                );
            }
            best_ms = best_ms.min(reply["decodeComputeMs"].as_u64().unwrap_or(u64::MAX));
            if best_ms < limit {
                break;
            }
        }
        if index == 0 {
            timings.short_ms = Some(best_ms);
        } else {
            timings.long_ms = Some(best_ms);
        }
        if best_ms >= limit {
            return (
                Verdict::Retry(format!(
                    "correct but slow: {} took {best_ms} ms (bar {limit} ms)",
                    clip.name
                )),
                timings,
            );
        }
    }
    (Verdict::Passed, timings)
}

/// faster-whisper on the CPU for the same clips: upgraders keep it as their
/// CPU rung, so the GPU path must beat it.
async fn faster_whisper_baseline(runtime_model: &str, clips: &[QualifyClip]) -> Option<Vec<u64>> {
    let cancel = CancellationToken::new();
    let mut times = Vec::new();
    for clip in clips {
        let overrides = FasterWhisperRequestOverrides {
            beam_size: Some(QUALIFY_BEAM),
            ..Default::default()
        };
        match transcribe_samples_with_overrides(
            &clip.samples,
            runtime_model,
            DecodeMode::Balanced,
            overrides,
            &cancel,
        )
        .await
        {
            Ok(output) => times.push(output.decode_compute_ms),
            Err(err) => {
                tracing::warn!("qualification skipped the faster-whisper comparison: {err}");
                return None;
            }
        }
    }
    Some(times)
}

fn now_utc_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|age| age.as_millis() as u64)
        .unwrap_or(0)
}

/// Qualification: the worker's first start with the user's real model on
/// the chosen GPU (plan 019). Warm-up, then two bundled test clips; passes
/// when the GPU backend actually ran, both transcripts are right, the warm
/// decodes beat the bars, and (with `fw_baseline`, the installed
/// faster-whisper model's runtime id) faster-whisper on the CPU is slower.
/// Pass and fail are cached by key; slow, out-of-memory and cold-cache
/// timeouts mean "CPU this session, retry next session", and the third such
/// result in a row is cached as a fail. On pass the worker
/// stays loaded as the live dictation worker. Serialized with polish prewarm
/// by the caller-shared `HEAVY_LOAD` lock. Returns `None` when there is
/// nothing to qualify or another qualification is already running.
pub async fn qualify(model: AsrModel, fw_baseline: Option<String>) -> Option<QualOutcome> {
    if !GPU_ENABLED.load(Ordering::Relaxed) {
        return None;
    }
    let device = gpu_select::hardware_profile_async().await.gpu?;
    let key = {
        let device = device.clone();
        let model = model.clone();
        tokio::task::spawn_blocking(move || qualification_key(&device, &model))
            .await
            .ok()?
    };
    if let Some(cached) = gpu_select::cached_qualification(&key) {
        return Some(cached);
    }
    {
        let mut session = lock(&SESSION);
        if session.qualifying.is_some() {
            return None;
        }
        if let Some((_, _)) = session.retry_next_session.iter().find(|(k, _)| *k == key) {
            return Some(QualOutcome::Retry);
        }
        session.qualifying = Some(key.clone());
    }
    let _heavy = super::HEAVY_LOAD.lock().await;
    tracing::info!(
        "qualifying '{}' on '{}' (first GPU use can take up to a minute)",
        model.id, device.name
    );

    let clips = load_qualify_clips();
    let (mut verdict, timings, clips) = match clips {
        Ok(clips) => {
            let model = model.clone();
            match tokio::task::spawn_blocking(move || {
                let (verdict, timings) = qualify_gpu_blocking(&model, &clips);
                (verdict, timings, clips)
            })
            .await
            {
                Ok(result) => result,
                Err(err) => (
                    Verdict::Retry(format!("qualification task failed: {err}")),
                    QualifyTimings::default(),
                    Vec::new(),
                ),
            }
        }
        Err(err) => (Verdict::Inconclusive(err), QualifyTimings::default(), Vec::new()),
    };

    let mut cpu_times = None;
    if let (Verdict::Passed, Some(runtime_model)) = (&verdict, fw_baseline.as_deref()) {
        cpu_times = faster_whisper_baseline(runtime_model, &clips).await;
        if let (Some(cpu), Some(short), Some(long)) = (&cpu_times, timings.short_ms, timings.long_ms) {
            // Compare totals, not each clip: a near-tie on one clip (seen on
            // the long clip with fast CPUs) must not flip the verdict.
            if short + long >= cpu[0] + cpu[1] {
                verdict = Verdict::Retry(format!(
                    "not faster than faster-whisper on the processor ({short}/{long} ms vs {}/{} ms)",
                    cpu[0], cpu[1]
                ));
            }
        }
    }

    let (outcome, detail) = match verdict {
        Verdict::Passed => (QualOutcome::Passed, "passed".to_string()),
        Verdict::Failed(reason) => (QualOutcome::Failed, trim_reason(&reason)),
        Verdict::Retry(reason)
            if gpu_select::consecutive_retries(&key) + 1 >= QUALIFY_MAX_RETRIES =>
        {
            (
                QualOutcome::Failed,
                trim_reason(&format!(
                    "no pass in {QUALIFY_MAX_RETRIES} checks in a row; last: {reason}"
                )),
            )
        }
        Verdict::Retry(reason) => (QualOutcome::Retry, trim_reason(&reason)),
        Verdict::Inconclusive(reason) => (
            QualOutcome::Retry,
            trim_reason(&format!("{}{reason}", gpu_select::INCONCLUSIVE_PREFIX)),
        ),
    };
    tracing::info!(
        "qualification {outcome:?} for '{}' on '{}' (load {} ms, clips {:?}/{:?} ms): {detail}",
        model.id, device.name, timings.load_ms, timings.short_ms, timings.long_ms
    );
    gpu_select::record_qualification(QualificationRecord {
        key: key.clone(),
        outcome,
        model_id: model.id.clone(),
        device_name: device.name.clone(),
        at_utc_ms: now_utc_ms(),
        detail: detail.clone(),
        load_ms: timings.load_ms,
        short_clip_ms: timings.short_ms,
        long_clip_ms: timings.long_ms,
        cpu_short_clip_ms: cpu_times.as_ref().map(|times| times[0]),
        cpu_long_clip_ms: cpu_times.as_ref().map(|times| times[1]),
    });
    if outcome != QualOutcome::Passed {
        // Free the VRAM; this session decodes on the CPU rung.
        if let Ok(mut guard) = GPU_SLOT.try_lock() {
            *guard = None;
        }
    }
    let mut session = lock(&SESSION);
    session.qualifying = None;
    if outcome == QualOutcome::Retry {
        session.retry_next_session.push((key, detail));
    }
    Some(outcome)
}

/// Run qualification in the background (dictation path).
pub(crate) fn spawn_qualification(
    model: AsrModel,
    fw_baseline: Option<String>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let _ = qualify(model, fw_baseline).await;
    })
}

/// Engine state for the UI ("Running on …", "Getting your graphics card
/// ready") for an active whisper.cpp model.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AsrEngineView {
    pub running_on: Option<RunningOn>,
    /// The one-time GPU check for this model is running.
    pub preparing: bool,
    pub qualification: Option<QualOutcome>,
    /// Detail of the latest qualification (diagnostics, settings tooltip).
    pub qualification_detail: Option<String>,
}

pub fn engine_view(model: &AsrModel) -> AsrEngineView {
    let device = hardware_profile().gpu;
    let key = device
        .as_ref()
        .map(|device| qualification_key(device, model));
    let latest = key.as_deref().and_then(gpu_select::latest_qualification);
    let preparing = {
        let session = lock(&SESSION);
        key.is_some() && session.qualifying == key
    };
    let (rung, _) = primary_rung_blocking(model);
    let report = lock(&SESSION)
        .last_report
        .clone()
        .filter(|(path, _)| *path == model.path)
        .map(|(_, running)| running);
    // The worker's own report wins once it decoded this model on the rung
    // the next dictation will use; before that, predict from the rung.
    let running_on = match (rung, report) {
        _ if preparing => None,
        (rung, Some(report)) if report.device == rung => Some(report),
        (Rung::Gpu, _) => Some(RunningOn {
            device: Rung::Gpu,
            name: device.map(|device| device.name),
        }),
        (Rung::Cpu, _) => Some(RunningOn {
            device: Rung::Cpu,
            name: None,
        }),
    };
    AsrEngineView {
        running_on,
        preparing,
        qualification: latest.as_ref().map(|record| record.outcome),
        qualification_detail: latest.map(|record| record.detail),
    }
}

fn join_error(err: tokio::task::JoinError) -> InferenceError {
    InferenceError::RuntimeJoin(format!("speech worker task failed: {err}"))
}

/// Primary rung for the next decode, and why it is not the GPU (see
/// `primary_rung_blocking`).
pub(crate) async fn primary_rung(model: &AsrModel) -> (Rung, Option<StepDown>) {
    let model = model.clone();
    tokio::task::spawn_blocking(move || primary_rung_blocking(&model))
        .await
        .unwrap_or((Rung::Cpu, None))
}

/// For a dictation whose only CPU rung is the CPU worker: wait for the
/// one-time GPU check on `model` instead of cold-loading the model on the
/// processor next to it (31 s for Large Turbo, and it slowed the check's GPU
/// load from ~27 s to ~50 s). Starts the check when none is running (a model
/// switch can leave another model's check in flight, or none at all). Returns
/// the re-evaluated primary rung once the check is done (on pass the GPU
/// worker stays loaded), after `GPU_CHECK_WAIT`, or on cancel.
pub(crate) async fn wait_for_gpu_check(
    model: &AsrModel,
    cancel: &CancellationToken,
) -> (Rung, Option<StepDown>) {
    let started = Instant::now();
    let mut check: Option<tokio::task::JoinHandle<()>> = None;
    loop {
        let (rung, step_down) = primary_rung(model).await;
        let preparing = step_down
            .as_ref()
            .is_some_and(|step| step.kind == StepDownKind::Preparing);
        if cancel.is_cancelled() {
            return (rung, step_down);
        }
        if !preparing || started.elapsed() >= GPU_CHECK_WAIT {
            tracing::info!(
                "dictation waited {} ms for the GPU check; decoding on the {} worker",
                started.elapsed().as_millis(),
                rung.dir_name()
            );
            return (rung, step_down);
        }
        // One spawn at a time: a check spends a moment before it marks
        // itself as running, and a duplicate would only return early.
        let idle = lock(&SESSION).qualifying.is_none();
        if idle && check.as_ref().map_or(true, tokio::task::JoinHandle::is_finished) {
            check = Some(spawn_qualification(model.clone(), None));
        }
        let _ = tokio::time::timeout(GPU_CHECK_POLL, cancel.cancelled()).await;
    }
}

/// One decode on `rung`. `overrides` carries the shared pipeline's beam size
/// and prompt (Parakeet ignores both; it takes `boost_terms` instead, which
/// whisper models never send); the worker always decodes at temperature 0
/// without previous text conditioning. `load_deadline`
/// overrides the rung's default (the fresh-worker retry after a GPU fault
/// uses a short one).
pub(crate) async fn transcribe(
    rung: Rung,
    model: &AsrModel,
    load_deadline: Option<Duration>,
    samples: &[f32],
    decode_mode: DecodeMode,
    overrides: &FasterWhisperRequestOverrides,
    boost_terms: &[String],
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
        load_deadline: load_deadline.unwrap_or_else(|| self::load_deadline(rung)),
        audio_pcm16_b64: encode_pcm16_base64(samples),
        audio_ms: samples.len() as u64 * 1000 / 16_000,
        beam_size: overrides.beam_size.unwrap_or(default_beam).clamp(1, 8),
        initial_prompt: overrides
            .initial_prompt
            .as_deref()
            .map(str::trim)
            .filter(|prompt| !prompt.is_empty())
            .map(str::to_string),
        boost_terms: if model.is_parakeet() {
            boost_terms.to_vec()
        } else {
            Vec::new()
        },
    };
    let model = model.clone();
    let cancel = cancel.clone();
    tokio::task::spawn_blocking(move || transcribe_blocking(rung, &model, job, &cancel))
        .await
        .map_err(join_error)?
        .map_err(InferenceError::from)
}

/// Spawn and load the worker the next dictation will use, so it starts warm.
/// When the GPU path is not qualified for this model yet, this runs the
/// qualification instead (it leaves the worker loaded on pass). Serialized by
/// the caller against polish prewarm; returns once done.
pub async fn prewarm_asr_worker(
    model: AsrModel,
    fw_baseline: Option<String>,
) -> Result<(), InferenceError> {
    let prewarm_model = model.clone();
    let next = tokio::task::spawn_blocking(move || prewarm_blocking(&prewarm_model))
        .await
        .map_err(join_error)?
        .map_err(InferenceError::from)?;
    if let Prewarm::Qualify = next {
        if qualify(model.clone(), fw_baseline).await == Some(QualOutcome::Passed) {
            return Ok(());
        }
        // Not qualified this session: warm the CPU rung instead (a no-op
        // when another qualification is still running).
        tokio::task::spawn_blocking(move || prewarm_blocking(&model))
            .await
            .map_err(join_error)?
            .map_err(InferenceError::from)?;
    }
    Ok(())
}

/// Drop `rung`'s worker if it is idle, freeing the VRAM/RAM its model holds.
/// A worker mid-load or mid-decode is left alone.
pub(crate) fn release_idle_rung(rung: Rung) {
    if let Ok(mut guard) = slot(rung).try_lock() {
        *guard = None;
    }
}

/// Drop workers that are idle (e.g. after switching to a faster-whisper
/// model).
pub fn release_idle_asr_workers() {
    for rung in [Rung::Gpu, Rung::Cpu] {
        release_idle_rung(rung);
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
