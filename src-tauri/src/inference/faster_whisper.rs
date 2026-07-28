use super::InferenceError;
use crate::settings::DecodeMode;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, RecvTimeoutError},
    Mutex, MutexGuard, OnceLock, TryLockError,
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio_util::sync::CancellationToken;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;
#[cfg(target_os = "windows")]
const DETACHED_PROCESS: u32 = 0x00000008;

#[derive(Debug, Clone)]
pub struct FasterWhisperDecodeOutput {
    pub text: String,
    pub model_init_ms: u64,
    pub decode_compute_ms: u64,
    pub runtime_cache_hit: bool,
    pub segment_count: u32,
    pub avg_logprob: f32,
    pub no_speech_prob: f32,
    pub compression_ratio: f32,
    pub backend_requested: String,
    pub backend_used: String,
    pub backend_fallback: bool,
}

#[derive(Debug, Clone)]
pub struct FasterWhisperPrefetchOutput {
    pub model_init_ms: u64,
    pub runtime_cache_hit: bool,
}

#[derive(Debug, Clone, Default)]
pub struct FasterWhisperRequestOverrides {
    pub beam_size: Option<u32>,
    pub best_of: Option<u32>,
    pub vad_filter: Option<bool>,
    pub condition_on_previous_text: Option<bool>,
    pub without_timestamps: Option<bool>,
    pub initial_prompt: Option<String>,
    pub temperature: Option<f32>,
    pub no_speech_threshold: Option<f32>,
    pub log_prob_threshold: Option<f32>,
    pub compression_ratio_threshold: Option<f32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkerRequest {
    id: u64,
    command: String,
    model_id: String,
    compute_type: String,
    backend_preference: String,
    allow_backend_fallback: bool,
    audio_path: String,
    audio_pcm16_b64: Option<String>,
    sample_rate_hz: u32,
    beam_size: u32,
    best_of: u32,
    language: String,
    vad_filter: bool,
    condition_on_previous_text: bool,
    without_timestamps: bool,
    initial_prompt: Option<String>,
    temperature: Option<f32>,
    no_speech_threshold: Option<f32>,
    log_prob_threshold: Option<f32>,
    compression_ratio_threshold: Option<f32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkerResponse {
    id: Option<u64>,
    ok: bool,
    error: Option<String>,
    text: Option<String>,
    #[serde(default)]
    model_init_ms: u64,
    #[serde(default)]
    decode_compute_ms: u64,
    #[serde(default)]
    runtime_cache_hit: bool,
    #[serde(default)]
    warmup_ms: u64,
    #[serde(default)]
    segment_count: u32,
    #[serde(default)]
    avg_log_prob: f32,
    #[serde(default)]
    no_speech_prob: f32,
    #[serde(default)]
    compression_ratio: f32,
    #[serde(default)]
    backend_requested: String,
    #[serde(default)]
    backend_used: String,
    #[serde(default)]
    backend_fallback: bool,
}

struct WorkerProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: Option<BufReader<ChildStdout>>,
    next_id: u64,
}

/// Killing on drop is deliberate: the worker is spawned DETACHED_PROCESS, so a
/// handle dropped without an explicit kill (an error path, a panicking blocking
/// thread) leaves a python.exe running with nothing left that can reach it.
impl Drop for WorkerProcess {
    fn drop(&mut self) {
        unregister_worker_pid(self.child.id());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

static WORKER: OnceLock<Mutex<Option<WorkerProcess>>> = OnceLock::new();
/// PIDs of every live worker process. Needed because an in-flight request
/// checks the worker OUT of `WORKER` (see `checkout_worker`), so during the
/// first-run model download — exactly when cancelling or quitting has to reach
/// the process — the slot is empty and the pid is the only handle left.
static LIVE_WORKER_PIDS: Mutex<Vec<u32>> = Mutex::new(Vec::new());
/// Serializes worker requests. Without this, a dictation arriving while the
/// startup prefetch is still loading the model checks out an empty slot,
/// spawns a SECOND worker, and pays the full model load itself (observed as
/// 7-10 s first dictations) — plus two copies of the model on the GPU.
static WORKER_REQUEST_GATE: Mutex<()> = Mutex::new(());
/// At most one background worker reload at a time (see schedule_worker_reload).
static WORKER_RELOAD_IN_FLIGHT: AtomicBool = AtomicBool::new(false);
/// How long a request waits for the gate before proceeding ungated. Covers a
/// warm model load (~7 s) but not a first-time multi-minute model download —
/// degrading to the old racing behavior beats hanging a dictation that long.
const WORKER_GATE_MAX_WAIT: Duration = Duration::from_secs(15);
/// Last time the worker GPU did real work (decode or warmup). Used to skip
/// the start-of-dictation warmup when the GPU is already hot.
static LAST_GPU_ACTIVITY: Mutex<Option<Instant>> = Mutex::new(None);
/// GPUs drop clocks within seconds of going idle; past this window a
/// dictation-start warmup is worth the ~100-300 ms of background GPU work.
const GPU_WARMUP_IDLE_THRESHOLD: Duration = Duration::from_secs(10);
const FW_GPU_DISABLED_MARKER: &str = "fw-gpu-disabled.marker";
const FW_WORKER_RESPONSE_TIMEOUT_MS_DEFAULT: u64 = 75_000;
const FW_WORKER_RESPONSE_TIMEOUT_MS_MIN: u64 = 3_000;
const FW_WORKER_RESPONSE_TIMEOUT_MS_MAX: u64 = 300_000;
// Prefetch downloads the model weights from HuggingFace (up to ~3 GB for large-v3).
// Use a much longer ceiling so slow connections can finish the first-time download.
const FW_WORKER_PREFETCH_TIMEOUT_MS_DEFAULT: u64 = 1_800_000;
const FW_WORKER_PREFETCH_TIMEOUT_MS_MIN: u64 = 60_000;
const FW_WORKER_PREFETCH_TIMEOUT_MS_MAX: u64 = 7_200_000;

pub async fn ensure_faster_whisper_ready() -> Result<(), InferenceError> {
    tokio::task::spawn_blocking(ensure_worker_ready_blocking)
        .await
        .map_err(|err| {
            InferenceError::RuntimeJoin(format!("faster-whisper readiness join failure: {err}"))
        })?
}

fn prefetch_request_for(model_id: &str) -> WorkerRequest {
    let backend_preference = worker_backend_preference_for_model(model_id);
    let compute_type = worker_compute_type_for_backend(&backend_preference);
    WorkerRequest {
        id: next_request_id(),
        command: "prefetch".to_string(),
        model_id: model_id.to_string(),
        compute_type,
        backend_preference,
        allow_backend_fallback: true,
        audio_path: String::new(),
        audio_pcm16_b64: None,
        sample_rate_hz: 16_000,
        beam_size: 2,
        best_of: 1,
        language: "en".to_string(),
        vad_filter: false,
        condition_on_previous_text: false,
        without_timestamps: false,
        initial_prompt: None,
        temperature: None,
        no_speech_threshold: None,
        log_prob_threshold: None,
        compression_ratio_threshold: None,
    }
}

fn note_gpu_activity() {
    if let Ok(mut guard) = LAST_GPU_ACTIVITY.lock() {
        *guard = Some(Instant::now());
    }
}

fn gpu_recently_active() -> bool {
    LAST_GPU_ACTIVITY
        .lock()
        .ok()
        .and_then(|guard| *guard)
        .is_some_and(|at| at.elapsed() < GPU_WARMUP_IDLE_THRESHOLD)
}

/// Fire-and-forget GPU warmup for an imminent decode. Called when dictation
/// STARTS so clock ramp + any cold kernels are paid while the user is still
/// speaking, not added to the release-to-text latency. No-op when the GPU
/// did real work recently.
pub async fn warmup_model(model_id: &str) -> Result<(), InferenceError> {
    if gpu_recently_active() {
        return Ok(());
    }
    let mut request = prefetch_request_for(model_id);
    request.command = "warmup".to_string();
    let response = tokio::task::spawn_blocking(move || send_worker_request_blocking(request))
        .await
        .map_err(|err| {
            InferenceError::RuntimeJoin(format!("faster-whisper warmup join failure: {err}"))
        })??;
    if response.ok {
        note_gpu_activity();
    }
    Ok(())
}

pub async fn prefetch_model(model_id: &str) -> Result<FasterWhisperPrefetchOutput, InferenceError> {
    let request = prefetch_request_for(model_id);
    let response = tokio::task::spawn_blocking(move || send_worker_request_blocking(request))
        .await
        .map_err(|err| {
            InferenceError::RuntimeJoin(format!("faster-whisper prefetch join failure: {err}"))
        })??;

    if response.warmup_ms > 0 {
        // Fresh load: the worker ran its kernel-compilation warmup decode,
        // so the GPU is genuinely hot right now.
        note_gpu_activity();
    }
    Ok(FasterWhisperPrefetchOutput {
        model_init_ms: response.model_init_ms,
        runtime_cache_hit: response.runtime_cache_hit,
    })
}

pub async fn transcribe_samples_with_overrides(
    samples: &[f32],
    model_id: &str,
    decode_mode: DecodeMode,
    overrides: FasterWhisperRequestOverrides,
    cancel_token: &CancellationToken,
) -> Result<FasterWhisperDecodeOutput, InferenceError> {
    if cancel_token.is_cancelled() {
        return Err(InferenceError::Cancelled);
    }

    let audio_pcm16_b64 = encode_pcm16_base64(samples);
    let request = worker_request_for(model_id, decode_mode, overrides, audio_pcm16_b64);
    let result = tokio::task::spawn_blocking(move || send_worker_request_blocking(request))
        .await
        .map_err(|err| {
            InferenceError::RuntimeJoin(format!("faster-whisper worker join failure: {err}"))
        })?;

    if cancel_token.is_cancelled() {
        return Err(InferenceError::Cancelled);
    }
    let response = result?;
    note_gpu_activity();
    if should_manage_fw_gpu_disable_marker() {
        if response.backend_fallback
            && response.backend_requested.eq_ignore_ascii_case("cuda")
            && response.backend_used.eq_ignore_ascii_case("cpu")
        {
            set_fw_gpu_persistently_disabled(true);
        } else if response.backend_used.eq_ignore_ascii_case("cuda") {
            set_fw_gpu_persistently_disabled(false);
        }
    }
    Ok(FasterWhisperDecodeOutput {
        text: response.text.unwrap_or_default(),
        model_init_ms: response.model_init_ms,
        decode_compute_ms: response.decode_compute_ms,
        runtime_cache_hit: response.runtime_cache_hit,
        segment_count: response.segment_count,
        avg_logprob: response.avg_log_prob,
        no_speech_prob: response.no_speech_prob,
        compression_ratio: response.compression_ratio,
        backend_requested: response.backend_requested,
        backend_used: response.backend_used,
        backend_fallback: response.backend_fallback,
    })
}

/// Reap a worker that was spawned but never became a `WorkerProcess` (so its
/// kill-on-drop never applies) and drop its pid from the live registry.
fn kill_partial_worker(mut child: Child) {
    unregister_worker_pid(child.id());
    let _ = child.kill();
    let _ = child.wait();
}

fn register_worker_pid(pid: u32) {
    if let Ok(mut guard) = LIVE_WORKER_PIDS.lock() {
        if !guard.contains(&pid) {
            guard.push(pid);
        }
    }
}

fn unregister_worker_pid(pid: u32) {
    if let Ok(mut guard) = LIVE_WORKER_PIDS.lock() {
        guard.retain(|value| *value != pid);
    }
}

#[cfg(target_os = "windows")]
fn kill_process_by_pid(pid: u32) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};

    // SAFETY: opening a process we spawned ourselves; failure returns null.
    let handle = unsafe { OpenProcess(PROCESS_TERMINATE, 0, pid) };
    if handle.is_null() {
        return;
    }
    // SAFETY: handle came from OpenProcess above and is closed right after.
    unsafe {
        TerminateProcess(handle, 1);
        CloseHandle(handle);
    }
}

#[cfg(not(target_os = "windows"))]
fn kill_process_by_pid(_pid: u32) {}

/// Kill every live faster-whisper worker process.
///
/// Two callers: cancelling a model download (the prefetch is blocked inside the
/// worker's `WhisperModel()` call and never reads stdin, so killing the process
/// is the only way to abort it) and app shutdown (DETACHED_PROCESS workers
/// otherwise outlive the app and keep downloading in the background).
///
/// The worker's warm model cache dies with it; the next request respawns via
/// `ensure_worker` and pays one model load. That is the accepted tradeoff.
pub fn kill_worker_processes() {
    if let Some(slot) = WORKER.get() {
        if let Ok(mut guard) = slot.lock() {
            // Dropping the handle kills and unregisters it (see Drop above).
            drop(guard.take());
        }
    }
    let pids = LIVE_WORKER_PIDS
        .lock()
        .map(|mut guard| std::mem::take(&mut *guard))
        .unwrap_or_default();
    for pid in pids {
        kill_process_by_pid(pid);
    }
}

pub fn cache_hint_for_model(model_id: &str) -> PathBuf {
    // Must mirror faster_whisper.utils._MODELS: each runtime model id resolves
    // to a specific HF repo, and not all of them are published by Systran.
    // If this path doesn't match the real cache directory, install completion
    // and download progress silently never resolve (SourceMissing).
    let repo = match model_id {
        "large-v3-turbo" => "models--mobiuslabsgmbh--faster-whisper-large-v3-turbo".to_string(),
        _ => format!("models--Systran--faster-whisper-{model_id}"),
    };
    hf_home_dir().join("hub").join(repo)
}

fn ensure_worker_ready_blocking() -> Result<(), InferenceError> {
    let slot = WORKER.get_or_init(|| Mutex::new(None));
    let mut guard = slot.lock().map_err(|_| {
        InferenceError::RuntimeJoin("faster-whisper worker lock poisoned".to_string())
    })?;
    if guard
        .as_mut()
        .is_some_and(|worker| worker.child.try_wait().ok().flatten().is_none())
    {
        return Ok(());
    }
    *guard = Some(spawn_worker()?);
    Ok(())
}

/// Drain any complete lines already buffered inside a BufReader.
///
/// Defense in depth against stdout pollution between requests: if a prior
/// request finished after its caller had already given up (e.g., the async
/// side observed a cancel), the worker's response line may still be sitting
/// in the BufReader's internal buffer. The next request would then read
/// that stale response and hit the ID-mismatch branch, killing the worker
/// and forcing a fresh spawn (visible to the user as a 300-600 ms stall).
///
/// This function only consumes bytes that are ALREADY inside the BufReader's
/// buffer — it never issues a new read to the underlying pipe, so it cannot
/// block even if the pipe is idle. Returns the number of complete lines
/// drained; non-zero is a signal that something went out of sync.
fn drain_buffered_stdout_lines<R: Read>(reader: &mut BufReader<R>) -> usize {
    let mut drained = 0;
    loop {
        // Only loop while the internal buffer contains a complete line.
        // If there's a partial line (no newline) or the buffer is empty,
        // stop — reading further would block on the underlying pipe.
        if !reader.buffer().contains(&b'\n') {
            break;
        }
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() {
            break;
        }
        drained += 1;
    }
    drained
}

fn send_worker_request_blocking(request: WorkerRequest) -> Result<WorkerResponse, InferenceError> {
    let _gate = acquire_request_gate(WORKER_GATE_MAX_WAIT);
    let command = request.command.clone();
    let model_id = request.model_id.clone();
    let result = send_worker_request_inner(request);
    // Every RuntimeJoin error path below leaves the worker dead (killed or
    // already exited). Reload it in the background now so the user's NEXT
    // dictation runs warm instead of paying the ~7 s model load inline.
    // Prefetches are excluded: a failing reload must not reschedule itself.
    if command != "prefetch" && matches!(&result, Err(InferenceError::RuntimeJoin(_))) {
        schedule_worker_reload(model_id);
    }
    result
}

fn acquire_request_gate(max_wait: Duration) -> Option<MutexGuard<'static, ()>> {
    let deadline = Instant::now() + max_wait;
    loop {
        match WORKER_REQUEST_GATE.try_lock() {
            Ok(guard) => return Some(guard),
            Err(TryLockError::WouldBlock) => {
                if Instant::now() >= deadline {
                    eprintln!(
                        "voicewave: worker request gate busy for {} ms; proceeding ungated",
                        max_wait.as_millis()
                    );
                    return None;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            // A poisoned gate only means a holder panicked; serialization is
            // best-effort, so proceed ungated rather than failing the decode.
            Err(TryLockError::Poisoned(_)) => return None,
        }
    }
}

fn schedule_worker_reload(model_id: String) {
    if WORKER_RELOAD_IN_FLIGHT
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("voicewave-fw-reload".to_string())
        .spawn(move || {
            eprintln!("voicewave: worker died; reloading {model_id} in background");
            match send_worker_request_blocking(prefetch_request_for(&model_id)) {
                Ok(response) => eprintln!(
                    "voicewave: background worker reload complete for {model_id} (model_init={} ms)",
                    response.model_init_ms
                ),
                Err(err) => {
                    eprintln!("voicewave: background worker reload failed for {model_id}: {err}")
                }
            }
            WORKER_RELOAD_IN_FLIGHT.store(false, Ordering::SeqCst);
        });
    if spawned.is_err() {
        WORKER_RELOAD_IN_FLIGHT.store(false, Ordering::SeqCst);
    }
}

fn send_worker_request_inner(request: WorkerRequest) -> Result<WorkerResponse, InferenceError> {
    let mut worker = checkout_worker()?;

    // Before writing a new request, drain any stale response lines that a
    // prior cancelled request may have left buffered. See
    // drain_buffered_stdout_lines docs for the race this protects against.
    if let Some(reader) = worker.stdout.as_mut() {
        let drained = drain_buffered_stdout_lines(reader);
        if drained > 0 {
            eprintln!(
                "voicewave: drained {drained} stale worker response line(s) before request {}",
                request.id
            );
        }
    }

    let payload = serde_json::to_string(&request).map_err(|err| {
        InferenceError::RuntimeJoin(format!("encode worker request failed: {err}"))
    })?;
    worker
        .stdin
        .write_all(payload.as_bytes())
        .map_err(|err| InferenceError::RuntimeJoin(format!("worker stdin write failed: {err}")))?;
    worker.stdin.write_all(b"\n").map_err(|err| {
        InferenceError::RuntimeJoin(format!("worker stdin newline write failed: {err}"))
    })?;
    worker
        .stdin
        .flush()
        .map_err(|err| InferenceError::RuntimeJoin(format!("worker stdin flush failed: {err}")))?;

    let timeout = if request.command == "prefetch" {
        worker_prefetch_timeout()
    } else {
        worker_response_timeout()
    };
    let line = match read_worker_response_line_with_timeout(&mut worker, timeout) {
        Ok(value) => value,
        Err(err) => {
            let _ = worker.child.kill();
            let _ = worker.child.wait();
            return Err(err);
        }
    };

    let response: WorkerResponse = match serde_json::from_str(line.trim()) {
        Ok(value) => value,
        Err(err) => {
            let _ = worker.child.kill();
            let _ = worker.child.wait();
            return Err(InferenceError::RuntimeJoin(format!(
                "parse worker response failed: {err}"
            )));
        }
    };
    if !response.ok {
        let reason = response
            .error
            .unwrap_or_else(|| "faster-whisper request failed".to_string());
        checkin_worker(worker);
        return Err(InferenceError::DecodeFailed {
            model_id: request.model_id,
            reason,
        });
    }
    if response.id != Some(request.id) {
        let _ = worker.child.kill();
        let _ = worker.child.wait();
        return Err(InferenceError::RuntimeJoin(format!(
            "worker response id mismatch (expected {}, got {:?})",
            request.id, response.id
        )));
    }
    checkin_worker(worker);
    Ok(response)
}

fn checkout_worker() -> Result<WorkerProcess, InferenceError> {
    let slot = WORKER.get_or_init(|| Mutex::new(None));
    let mut guard = slot.lock().map_err(|_| {
        InferenceError::RuntimeJoin("faster-whisper worker lock poisoned".to_string())
    })?;
    let _ = ensure_worker(&mut guard)?;
    guard.take().ok_or_else(|| {
        InferenceError::RuntimeJoin("failed to acquire faster-whisper worker".to_string())
    })
}

fn checkin_worker(mut worker: WorkerProcess) {
    let slot = WORKER.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = slot.lock() {
        if guard.is_none() {
            *guard = Some(worker);
            return;
        }
    }
    let _ = worker.child.kill();
    let _ = worker.child.wait();
}

fn worker_response_timeout() -> Duration {
    let timeout_ms = std::env::var("VOICEWAVE_FW_WORKER_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|value| {
            value.clamp(
                FW_WORKER_RESPONSE_TIMEOUT_MS_MIN,
                FW_WORKER_RESPONSE_TIMEOUT_MS_MAX,
            )
        })
        .unwrap_or(FW_WORKER_RESPONSE_TIMEOUT_MS_DEFAULT);
    Duration::from_millis(timeout_ms)
}

fn worker_prefetch_timeout() -> Duration {
    let timeout_ms = std::env::var("VOICEWAVE_FW_PREFETCH_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|value| {
            value.clamp(
                FW_WORKER_PREFETCH_TIMEOUT_MS_MIN,
                FW_WORKER_PREFETCH_TIMEOUT_MS_MAX,
            )
        })
        .unwrap_or(FW_WORKER_PREFETCH_TIMEOUT_MS_DEFAULT);
    Duration::from_millis(timeout_ms)
}

fn read_worker_response_line_with_timeout(
    worker: &mut WorkerProcess,
    timeout: Duration,
) -> Result<String, InferenceError> {
    let stdout = worker
        .stdout
        .take()
        .ok_or_else(|| InferenceError::RuntimeJoin("worker stdout unavailable".to_string()))?;
    let (tx, rx) = mpsc::channel();

    // Read worker stdout on a helper thread so stalled GPU decode cannot block forever.
    std::thread::spawn(move || {
        let mut stdout = stdout;
        let mut line = String::new();
        let result = match stdout.read_line(&mut line) {
            Ok(bytes) => Ok((stdout, bytes, line)),
            Err(err) => Err((stdout, err)),
        };
        let _ = tx.send(result);
    });

    match rx.recv_timeout(timeout) {
        Ok(Ok((stdout, bytes, line))) => {
            worker.stdout = Some(stdout);
            if bytes == 0 {
                return Err(InferenceError::RuntimeJoin(
                    "faster-whisper worker exited unexpectedly".to_string(),
                ));
            }
            Ok(line)
        }
        Ok(Err((stdout, err))) => {
            worker.stdout = Some(stdout);
            Err(InferenceError::RuntimeJoin(format!(
                "worker stdout read failed: {err}"
            )))
        }
        Err(RecvTimeoutError::Timeout) => {
            let timeout_ms = timeout.as_millis();
            let pid = worker.child.id();
            let _ = worker.child.kill();
            let _ = worker.child.wait();
            Err(InferenceError::RuntimeJoin(format!(
                "faster-whisper worker timed out after {timeout_ms} ms waiting for decode result (pid {pid}); worker restarted"
            )))
        }
        Err(RecvTimeoutError::Disconnected) => {
            let _ = worker.child.kill();
            let _ = worker.child.wait();
            Err(InferenceError::RuntimeJoin(
                "faster-whisper worker response channel disconnected".to_string(),
            ))
        }
    }
}

fn ensure_worker<'a>(
    guard: &'a mut Option<WorkerProcess>,
) -> Result<&'a mut WorkerProcess, InferenceError> {
    let needs_spawn = match guard.as_mut() {
        Some(worker) => worker.child.try_wait().ok().flatten().is_some(),
        None => true,
    };
    if needs_spawn {
        *guard = Some(spawn_worker()?);
    }
    guard.as_mut().ok_or_else(|| {
        InferenceError::RuntimeJoin("failed to start faster-whisper worker".to_string())
    })
}

fn spawn_worker() -> Result<WorkerProcess, InferenceError> {
    let worker_path = resolve_worker_path()?;
    let python = resolve_python_path()?;
    let hf_home = hf_home_dir();
    let hub_cache = hf_home.join("hub");
    fs::create_dir_all(&hub_cache).map_err(|err| {
        InferenceError::RuntimeJoin(format!(
            "failed to create faster-whisper cache directories: {err}"
        ))
    })?;

    let thread_cap = preferred_worker_thread_cap().to_string();

    let mut command = Command::new(&python);
    command
        .arg(worker_path.as_os_str())
        .env("HF_HOME", &hf_home)
        .env("HUGGINGFACE_HUB_CACHE", &hub_cache)
        .env("TRANSFORMERS_CACHE", &hub_cache)
        .env("OMP_NUM_THREADS", &thread_cap)
        .env("CT2_NUM_THREADS", &thread_cap)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(target_os = "windows")]
    {
        // Prevent python worker startup from flashing a console window in desktop builds.
        command.creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS);
    }
    let cuda_bins = resolve_cuda_bin_paths(&python);
    if !cuda_bins.is_empty() {
        let prepend = cuda_bins
            .iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join(";");
        if let Ok(existing_path) = std::env::var("PATH") {
            let combined = format!("{prepend};{existing_path}");
            command.env("PATH", combined);
        } else {
            command.env("PATH", prepend);
        }
    }
    #[cfg(windows)]
    {
        // Prevent python.exe from flashing a console window when invoked from the Tauri GUI.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn().map_err(|err| {
        InferenceError::RuntimeJoin(format!(
            "failed to spawn faster-whisper worker using '{python}': {err}. Reinstall the latest beta build or configure VOICEWAVE_FASTER_WHISPER_PYTHON."
        ))
    })?;
    register_worker_pid(child.id());

    // Every early return below must reap the child: it is detached, so a
    // dropped `Child` handle would leave a live python.exe behind.
    let stdin = match child.stdin.take() {
        Some(value) => value,
        None => {
            kill_partial_worker(child);
            return Err(InferenceError::RuntimeJoin(
                "worker stdin unavailable".to_string(),
            ));
        }
    };
    let stdout = match child.stdout.take() {
        Some(value) => value,
        None => {
            kill_partial_worker(child);
            return Err(InferenceError::RuntimeJoin(
                "worker stdout unavailable".to_string(),
            ));
        }
    };
    let mut stdout_reader = BufReader::new(stdout);

    let mut ready_line = String::new();
    let ready_bytes = match stdout_reader.read_line(&mut ready_line) {
        Ok(value) => value,
        Err(err) => {
            kill_partial_worker(child);
            return Err(InferenceError::RuntimeJoin(format!(
                "worker ready read failed: {err}"
            )));
        }
    };
    if ready_bytes == 0 {
        let stderr = child
            .stderr
            .take()
            .map(|mut row| {
                let mut out = String::new();
                let _ = row.read_to_string(&mut out);
                out
            })
            .unwrap_or_default();
        kill_partial_worker(child);
        return Err(InferenceError::RuntimeJoin(format!(
            "faster-whisper worker exited before ready. stderr: {}",
            stderr.trim()
        )));
    }

    Ok(WorkerProcess {
        child,
        stdin,
        stdout: Some(stdout_reader),
        next_id: 1,
    })
}

fn resolve_worker_path() -> Result<PathBuf, InferenceError> {
    if let Ok(path) = std::env::var("VOICEWAVE_FASTER_WHISPER_WORKER") {
        let value = PathBuf::from(path.trim());
        if value.exists() {
            return Ok(value);
        }
    }

    let candidates = worker_path_candidates();
    for candidate in candidates {
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    Err(InferenceError::RuntimeJoin(
        "faster-whisper worker is missing from this install. Reinstall the latest beta build or configure VOICEWAVE_FASTER_WHISPER_WORKER.".to_string(),
    ))
}

fn resolve_python_path() -> Result<String, InferenceError> {
    if let Ok(path) = std::env::var("VOICEWAVE_FASTER_WHISPER_PYTHON") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            let candidate = PathBuf::from(trimmed);
            if candidate.exists() {
                return Ok(candidate.to_string_lossy().to_string());
            }
        }
    }

    for candidate in python_path_candidates() {
        if candidate.exists() {
            return Ok(candidate.to_string_lossy().to_string());
        }
    }

    Err(InferenceError::RuntimeJoin(
        "faster-whisper python runtime is missing from this install. Reinstall the latest beta build or configure VOICEWAVE_FASTER_WHISPER_PYTHON.".to_string(),
    ))
}

fn worker_path_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(exe_dir) = current_exe_dir() {
        candidates.push(exe_dir.join("faster-whisper").join("worker.py"));
        candidates.push(
            exe_dir
                .join("resources")
                .join("faster-whisper")
                .join("worker.py"),
        );
    }

    if let Some(app_support_dir) = app_support_dir() {
        candidates.push(app_support_dir.join("faster-whisper").join("worker.py"));
    }

    candidates.push(
        PathBuf::from("scripts")
            .join("faster_whisper")
            .join("worker.py"),
    );
    candidates.push(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("scripts")
            .join("faster_whisper")
            .join("worker.py"),
    );

    candidates
}

fn python_path_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(exe_dir) = current_exe_dir() {
        candidates.extend(python_layout_candidates(&exe_dir.join("faster-whisper")));
        candidates.extend(python_layout_candidates(
            &exe_dir.join("resources").join("faster-whisper"),
        ));
    }

    if let Some(app_support_dir) = app_support_dir() {
        candidates.extend(python_layout_candidates(&app_support_dir.join("faster-whisper")));
    }

    candidates.extend(python_layout_candidates(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(".venv-faster-whisper"),
    ));

    candidates.extend(installed_system_python_candidates());
    dedupe_existing_paths(candidates)
}

fn python_layout_candidates(root: &Path) -> Vec<PathBuf> {
    vec![
        root.join("Scripts").join("python.exe"),
        root.join("python.exe"),
        root.join("python").join("Scripts").join("python.exe"),
        root.join("python").join("python.exe"),
    ]
}

fn installed_system_python_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        let programs_dir = PathBuf::from(local_app_data).join("Programs").join("Python");
        if let Ok(entries) = fs::read_dir(programs_dir) {
            let mut installs = entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| {
                    path.file_name()
                        .and_then(|value| value.to_str())
                        .is_some_and(|value| value.starts_with("Python"))
                })
                .collect::<Vec<_>>();
            installs.sort_by(|left, right| right.cmp(left));
            for install in installs {
                candidates.push(install.join("python.exe"));
            }
        }
    }

    let mut where_cmd = Command::new("where.exe");
    where_cmd.arg("python");
    #[cfg(target_os = "windows")]
    {
        where_cmd.creation_flags(CREATE_NO_WINDOW);
    }
    if let Ok(output) = where_cmd.output() {
        if output.status.success() {
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.contains("\\WindowsApps\\") {
                    continue;
                }
                candidates.push(PathBuf::from(trimmed));
            }
        }
    }

    candidates
}

fn dedupe_existing_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    paths.into_iter()
        .filter(|path| seen.insert(path.to_string_lossy().to_ascii_lowercase()))
        .collect()
}

fn current_exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
}

fn app_support_dir() -> Option<PathBuf> {
    ProjectDirs::from("com", "voicewave", "localcore")
        .map(|dirs| dirs.data_dir().join("runtime-support"))
}

fn resolve_cuda_bin_path() -> Option<PathBuf> {
    if let Ok(cuda_root) = std::env::var("CUDA_PATH") {
        let root = PathBuf::from(cuda_root.trim());
        let bin_x64 = root.join("bin").join("x64");
        if bin_x64.exists() {
            return Some(bin_x64);
        }
        let bin = root.join("bin");
        if bin.exists() {
            return Some(bin);
        }
    }

    let default_root = Path::new("C:\\Program Files\\NVIDIA GPU Computing Toolkit\\CUDA");
    if !default_root.exists() {
        return None;
    }
    let mut versions = fs::read_dir(default_root)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            entry.file_type().ok().and_then(|ft| {
                if ft.is_dir() {
                    Some(entry.path())
                } else {
                    None
                }
            })
        })
        .collect::<Vec<_>>();
    versions.sort_by(|a, b| b.cmp(a));
    versions
        .into_iter()
        .flat_map(|path| {
            let bin = path.join("bin");
            [bin.join("x64"), bin]
        })
        .find(|bin| bin.exists())
}

fn resolve_cuda_bin_paths(python_path: &str) -> Vec<PathBuf> {
    let mut paths = resolve_python_cuda_bin_paths(python_path);
    if let Some(cuda_bin) = resolve_cuda_bin_path() {
        paths.push(cuda_bin);
    }

    let mut dedup = HashSet::new();
    paths
        .into_iter()
        .filter(|path| path.exists())
        .filter(|path| dedup.insert(path.clone()))
        .collect()
}

fn resolve_python_cuda_bin_paths(python_path: &str) -> Vec<PathBuf> {
    let python = PathBuf::from(python_path.trim());
    let scripts_dir = match python.parent() {
        Some(value) => value,
        None => return Vec::new(),
    };
    let venv_root = match scripts_dir.parent() {
        Some(value) => value,
        None => return Vec::new(),
    };

    let nvidia_root = venv_root.join("Lib").join("site-packages").join("nvidia");
    if !nvidia_root.exists() {
        return Vec::new();
    }

    fs::read_dir(&nvidia_root)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            entry.file_type().ok().and_then(|ft| {
                if ft.is_dir() {
                    let bin = entry.path().join("bin");
                    if bin.exists() {
                        Some(bin)
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
        })
        .collect()
}

fn worker_request_for(
    model_id: &str,
    decode_mode: DecodeMode,
    overrides: FasterWhisperRequestOverrides,
    audio_pcm16_b64: String,
) -> WorkerRequest {
    let backend_preference = worker_backend_preference_for_model(model_id);
    let compute_type = worker_compute_type_for_backend(&backend_preference);
    let (beam_size, best_of) = decode_hyperparams_for(model_id, decode_mode);
    let initial_prompt = overrides
        .initial_prompt
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    WorkerRequest {
        id: next_request_id(),
        command: "transcribe".to_string(),
        model_id: model_id.to_string(),
        compute_type,
        backend_preference,
        allow_backend_fallback: true,
        audio_path: String::new(),
        audio_pcm16_b64: Some(audio_pcm16_b64),
        sample_rate_hz: 16_000,
        beam_size: overrides.beam_size.unwrap_or(beam_size),
        best_of: overrides.best_of.unwrap_or(best_of),
        language: "en".to_string(),
        // Push-to-talk path already endpoints speech; avoid double-VAD trimming.
        vad_filter: overrides.vad_filter.unwrap_or(false),
        condition_on_previous_text: overrides.condition_on_previous_text.unwrap_or(false),
        without_timestamps: overrides
            .without_timestamps
            .unwrap_or_else(fw_without_timestamps_enabled),
        initial_prompt,
        // Lock temperature to 0 (greedy / deterministic beam search). The
        // faster-whisper default is a fallback ladder [0, 0.2, 0.4, 0.6, 0.8, 1.0]
        // that kicks in when the primary decode "fails" a sanity check. Those
        // high-temperature retries are the main source of hallucinations on
        // short utterances because they invent words when the model is uncertain.
        temperature: Some(overrides.temperature.unwrap_or(0.0)),
        // Raise the no-speech confidence needed to blank out a segment so short
        // valid words are not dropped into empty strings.
        no_speech_threshold: Some(overrides.no_speech_threshold.unwrap_or(0.72)),
        // Floor on mean token log-probability of the decoded segment.
        // Low-confidence hallucinations (background hum decoded as "thank
        // you for watching", TV bleed-through, etc.) are coherent enough
        // to slip past the no_speech and compression_ratio guards because
        // they are grammatical but low-probability. A -1.0 floor rejects
        // those segments and returns empty text instead.
        log_prob_threshold: Some(overrides.log_prob_threshold.unwrap_or(-1.0)),
        // Tighter compression-ratio ceiling flags repeat-word hallucinations
        // ("the the the...") so the decoder rejects them instead of pasting.
        compression_ratio_threshold: Some(
            overrides.compression_ratio_threshold.unwrap_or(2.2),
        ),
    }
}

fn worker_backend_preference_for_model(model_id: &str) -> String {
    let force_cpu = env_flag("VOICEWAVE_FORCE_CPU", false);
    let force_gpu = env_flag("VOICEWAVE_FORCE_GPU", false);
    let auto_gpu_enabled = env_flag("VOICEWAVE_AUTO_GPU", true);
    // By default, ignore the persistent GPU-disable marker so that GPU can be
    // retried on subsequent runs unless the user explicitly opts back into the
    // old sticky-disable behavior.
    let respect_disable_marker = env_flag("VOICEWAVE_RESPECT_FW_GPU_DISABLE_MARKER", false);
    let persistently_disabled = if respect_disable_marker {
        fw_gpu_persistently_disabled()
    } else {
        false
    };

    select_worker_backend_preference(
        model_id,
        force_cpu,
        force_gpu,
        persistently_disabled,
        auto_gpu_enabled,
    )
    .to_string()
}

fn select_worker_backend_preference(
    model_id: &str,
    force_cpu: bool,
    force_gpu: bool,
    persistently_disabled: bool,
    auto_gpu_enabled: bool,
) -> &'static str {
    if force_cpu {
        return "cpu";
    }
    if force_gpu {
        return "cuda";
    }
    if persistently_disabled || !auto_gpu_enabled {
        return "cpu";
    }
    if is_gpu_preferred_model(model_id) {
        return "auto";
    }
    "cpu"
}

fn worker_compute_type_for_backend(backend_preference: &str) -> String {
    if backend_preference.eq_ignore_ascii_case("cpu") {
        std::env::var("VOICEWAVE_FW_CPU_COMPUTE_TYPE")
            .map(|value| value.trim().to_string())
            .ok()
            .filter(|value| !value.is_empty())
            // int8 activations quantize the matmul inputs and amplify small
            // phoneme ambiguities into hallucinations ("error"->"header").
            // int8_float32 keeps the quantized weights for speed but runs
            // activations at full float32 precision, meaningfully improving
            // accuracy with only a small latency hit on modern CPUs.
            .unwrap_or_else(|| "int8_float32".to_string())
    } else {
        std::env::var("VOICEWAVE_FW_GPU_COMPUTE_TYPE")
            .map(|value| value.trim().to_string())
            .ok()
            .filter(|value| !value.is_empty())
            // Prefer stability-first default on mixed Windows CUDA setups.
            .unwrap_or_else(|| "int8".to_string())
    }
}

fn is_gpu_preferred_model(model_id: &str) -> bool {
    let normalized = model_id.trim().to_ascii_lowercase();
    !(normalized.starts_with("tiny") || normalized.starts_with("base"))
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

fn fw_gpu_persistently_disabled() -> bool {
    fw_gpu_disable_marker_path()
        .as_ref()
        .is_some_and(|path| path.exists())
}

fn should_manage_fw_gpu_disable_marker() -> bool {
    env_flag("VOICEWAVE_RESPECT_FW_GPU_DISABLE_MARKER", false)
}

fn set_fw_gpu_persistently_disabled(disabled: bool) {
    let Some(path) = fw_gpu_disable_marker_path() else {
        return;
    };
    if disabled {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(path, now_utc_ms().to_string());
    } else if path.exists() {
        let _ = fs::remove_file(path);
    }
}

fn fw_gpu_disable_marker_path() -> Option<PathBuf> {
    ProjectDirs::from("com", "voicewave", "localcore")
        .map(|dirs| dirs.config_dir().join(FW_GPU_DISABLED_MARKER))
}

fn preferred_worker_thread_cap() -> usize {
    if let Ok(value) = std::env::var("VOICEWAVE_FW_THREADS") {
        if let Ok(parsed) = value.trim().parse::<usize>() {
            if parsed > 0 {
                return parsed.clamp(2, 16);
            }
        }
    }

    let available = std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(4);
    thread_cap_for_cores(available)
}

/// Pure helper: given the machine's logical core count, return the number
/// of threads to hand to the faster-whisper worker for decode. Reserves
/// one core for the UI thread so the Windows taskbar / main window don't
/// stutter during the 500-1500 ms of CPU inference. Caps at 12 to avoid
/// diminishing returns and cache pressure on high-core machines.
fn thread_cap_for_cores(cores: usize) -> usize {
    cores.saturating_sub(1).clamp(2, 12)
}

fn fw_without_timestamps_enabled() -> bool {
    std::env::var("VOICEWAVE_FW_WITHOUT_TIMESTAMPS")
        .map(|value| {
            let normalized = value.trim().to_ascii_lowercase();
            !(normalized == "0" || normalized == "false" || normalized == "off")
        })
        .unwrap_or(true)
}

fn encode_pcm16_base64(samples: &[f32]) -> String {
    let mut bytes = Vec::with_capacity(samples.len().saturating_mul(2));
    for sample in samples {
        let clamped = sample.clamp(-1.0, 1.0);
        let pcm = (clamped * i16::MAX as f32) as i16;
        bytes.extend_from_slice(&pcm.to_le_bytes());
    }
    BASE64_STANDARD.encode(bytes)
}

fn decode_hyperparams_for(model_id: &str, decode_mode: DecodeMode) -> (u32, u32) {
    match (model_id, decode_mode) {
        ("small.en", DecodeMode::Fast) => (1, 1),
        ("small.en", DecodeMode::Balanced) => (5, 3),
        ("small.en", DecodeMode::Quality) => (5, 3),
        ("large-v3", DecodeMode::Fast) => (1, 1),
        ("large-v3", DecodeMode::Balanced) => (5, 3),
        ("large-v3", DecodeMode::Quality) => (5, 3),
        ("large-v3-turbo", DecodeMode::Fast) => (1, 1),
        ("large-v3-turbo", DecodeMode::Balanced) => (5, 3),
        ("large-v3-turbo", DecodeMode::Quality) => (5, 3),
        (_, DecodeMode::Fast) => (1, 1),
        (_, DecodeMode::Balanced) => (4, 3),
        (_, DecodeMode::Quality) => (5, 3),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        cache_hint_for_model, decode_hyperparams_for, drain_buffered_stdout_lines,
        encode_pcm16_base64, prefetch_request_for, select_worker_backend_preference,
        thread_cap_for_cores, worker_request_for, FasterWhisperRequestOverrides,
    };
    use crate::settings::DecodeMode;
    use std::io::{BufRead, BufReader, Cursor};
    use std::sync::Mutex;
    use std::time::Duration;

    #[test]
    fn request_gate_waits_then_proceeds_ungated() {
        // Use a local gate (not the global) so this test cannot interfere
        // with other tests touching the real request path.
        fn acquire(gate: &'static Mutex<()>, max_wait: Duration) -> bool {
            let deadline = std::time::Instant::now() + max_wait;
            loop {
                match gate.try_lock() {
                    Ok(_guard) => return true,
                    Err(std::sync::TryLockError::WouldBlock) => {
                        if std::time::Instant::now() >= deadline {
                            return false;
                        }
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(std::sync::TryLockError::Poisoned(_)) => return false,
                }
            }
        }
        static GATE: Mutex<()> = Mutex::new(());

        assert!(acquire(&GATE, Duration::from_millis(50)), "free gate must acquire");

        let held = GATE.lock().expect("hold gate");
        assert!(
            !acquire(&GATE, Duration::from_millis(50)),
            "held gate must time out and proceed ungated"
        );
        drop(held);
        assert!(acquire(&GATE, Duration::from_millis(50)), "released gate must acquire");
    }

    #[test]
    fn prefetch_request_is_a_prefetch_for_the_requested_model() {
        let request = prefetch_request_for("large-v3-turbo");
        assert_eq!(request.command, "prefetch");
        assert_eq!(request.model_id, "large-v3-turbo");
        assert!(request.audio_pcm16_b64.is_none());
    }

    #[test]
    fn cache_hint_matches_real_hf_repo_per_model() {
        // Install completion and download progress both watch this path. If it
        // doesn't match the directory faster-whisper actually downloads into,
        // installs hang forever on SourceMissing.
        let small = cache_hint_for_model("small.en");
        assert!(small
            .to_string_lossy()
            .ends_with("models--Systran--faster-whisper-small.en"));
        let large = cache_hint_for_model("large-v3");
        assert!(large
            .to_string_lossy()
            .ends_with("models--Systran--faster-whisper-large-v3"));
        let turbo = cache_hint_for_model("large-v3-turbo");
        assert!(
            turbo
                .to_string_lossy()
                .ends_with("models--mobiuslabsgmbh--faster-whisper-large-v3-turbo"),
            "large-v3-turbo is published by mobiuslabsgmbh, not Systran; got {turbo:?}"
        );
    }

    #[test]
    fn drain_consumes_complete_lines_already_in_bufreader() {
        // A prior cancelled request left two response lines buffered. The
        // drain must consume both so the next request reads its own reply.
        let data = b"{\"id\":1,\"ok\":true}\n{\"id\":2,\"ok\":true}\n";
        let mut reader = BufReader::new(Cursor::new(data.to_vec()));
        // Force the BufReader to fill its internal buffer so buffer() is populated.
        let _ = reader.fill_buf().expect("fill ok");
        let drained = drain_buffered_stdout_lines(&mut reader);
        assert_eq!(drained, 2, "both buffered lines must be drained");
    }

    #[test]
    fn drain_is_noop_when_buffer_is_empty() {
        // Normal case: worker is in sync, no stale data buffered. Drain
        // must return 0 immediately without blocking or touching the
        // underlying reader.
        let data: &[u8] = b"";
        let mut reader = BufReader::new(Cursor::new(data.to_vec()));
        let drained = drain_buffered_stdout_lines(&mut reader);
        assert_eq!(drained, 0);
    }

    #[test]
    fn drain_does_not_consume_partial_line_without_newline() {
        // Regression: drain must NEVER trigger a pipe read to complete a
        // partial line. If the BufReader has bytes but no newline, stop
        // immediately — issuing read_line would block on the worker's
        // stdout pipe, which is exactly what this helper is meant to avoid.
        let data = b"partial_response_no_newline_yet";
        let mut reader = BufReader::new(Cursor::new(data.to_vec()));
        let _ = reader.fill_buf().expect("fill ok");
        let drained = drain_buffered_stdout_lines(&mut reader);
        assert_eq!(drained, 0, "partial line must stay buffered, not drained");
    }

    #[test]
    fn drain_handles_mixed_complete_and_partial_lines() {
        // One complete line followed by a partial. Should drain the
        // complete one and leave the partial alone.
        let data = b"{\"stale\":true}\npartial_no_newline";
        let mut reader = BufReader::new(Cursor::new(data.to_vec()));
        let _ = reader.fill_buf().expect("fill ok");
        let drained = drain_buffered_stdout_lines(&mut reader);
        assert_eq!(drained, 1, "complete line drained, partial preserved");
    }

    #[test]
    fn thread_cap_reserves_one_core_for_ui_on_common_laptops() {
        // Regression: on a 4-core laptop the old tier table returned 4,
        // pinning all cores and causing Windows UI jank (taskbar stutter)
        // for the 500-1500 ms of CPU decode. We must always leave at least
        // one core free for the UI thread.
        assert_eq!(thread_cap_for_cores(4), 3, "4-core should use 3, leaving 1 for UI");
        assert_eq!(thread_cap_for_cores(6), 5, "6-core should use 5, leaving 1 for UI");
        assert_eq!(thread_cap_for_cores(8), 7, "8-core should use 7, leaving 1 for UI");
        assert_eq!(thread_cap_for_cores(10), 9, "10-core should use 9, leaving 1 for UI");
    }

    #[test]
    fn thread_cap_still_caps_at_twelve_on_high_core_machines() {
        // On 14+ core machines, more threads hit diminishing returns and
        // cache pressure. Keep the hard ceiling at 12 from the old logic.
        assert_eq!(thread_cap_for_cores(14), 12);
        assert_eq!(thread_cap_for_cores(24), 12);
        assert_eq!(thread_cap_for_cores(64), 12);
    }

    #[test]
    fn thread_cap_never_drops_below_two_even_on_tiny_machines() {
        // Floor: if someone runs on a 1-core VM or core count probe fails,
        // we still need at least 2 threads for the worker to be usable.
        assert_eq!(thread_cap_for_cores(0), 2);
        assert_eq!(thread_cap_for_cores(1), 2);
        assert_eq!(thread_cap_for_cores(2), 2);
    }

    #[test]
    fn fw_balanced_profile_has_quality_floor_for_small() {
        let (beam, best_of) = decode_hyperparams_for("small.en", DecodeMode::Balanced);
        // Users saw roughly one minor error per sentence on Balanced.
        // Raise the floor so the default decode uses wider beam search.
        assert!(beam >= 4, "beam was {beam}, expected at least 4");
        assert!(best_of >= 3, "best_of was {best_of}, expected at least 3");
    }

    #[test]
    fn fw_balanced_profile_has_quality_floor_for_large() {
        let (beam, best_of) = decode_hyperparams_for("large-v3", DecodeMode::Balanced);
        assert!(beam >= 5, "beam was {beam}, expected at least 5");
        assert!(best_of >= 3, "best_of was {best_of}, expected at least 3");
    }

    #[test]
    fn fw_fast_profile_remains_latency_first() {
        let (beam, best_of) = decode_hyperparams_for("small.en", DecodeMode::Fast);
        assert_eq!(beam, 1);
        assert_eq!(best_of, 1);
    }

    #[test]
    fn fw_balanced_request_uses_plain_decode_without_prompt_or_context() {
        let request = worker_request_for(
            "small.en",
            DecodeMode::Balanced,
            FasterWhisperRequestOverrides::default(),
            "AQID".to_string(),
        );
        assert!(!request.condition_on_previous_text);
        assert!(request.initial_prompt.is_none());
        assert!(request.audio_pcm16_b64.is_some());
        assert_eq!(request.sample_rate_hz, 16_000);
        assert!(request.allow_backend_fallback);
        assert!(!request.backend_preference.is_empty());
        assert_eq!(
            request.without_timestamps,
            super::fw_without_timestamps_enabled()
        );
    }

    #[test]
    fn fw_request_defaults_suppress_hallucination_fallback() {
        // Hallucinations mostly come from the faster-whisper temperature
        // fallback ladder. The default request must lock temperature to 0,
        // keep a strong no_speech threshold, and cap compression ratio so
        // repeat-word hallucinations are rejected instead of pasted.
        let request = worker_request_for(
            "small.en",
            DecodeMode::Balanced,
            FasterWhisperRequestOverrides::default(),
            "AQID".to_string(),
        );
        assert_eq!(request.temperature, Some(0.0));
        assert!(
            request.no_speech_threshold.unwrap_or(0.0) >= 0.70,
            "no_speech_threshold was {:?}, expected >= 0.70",
            request.no_speech_threshold
        );
        assert!(
            request
                .compression_ratio_threshold
                .unwrap_or(f32::MAX)
                <= 2.3,
            "compression_ratio_threshold was {:?}, expected <= 2.3",
            request.compression_ratio_threshold
        );
        // log_prob_threshold puts a floor on the mean token log-probability
        // of the decoded segment. Low-confidence decodes (background hum
        // interpreted as "thank you for watching", TV bleed-through etc.)
        // score below -1.0. Without this floor they slip past the
        // no_speech and compression_ratio guards because they are coherent
        // but low-probability. Must be Some(value) <= -0.8.
        assert!(
            request.log_prob_threshold.unwrap_or(f32::MAX) <= -0.8,
            "log_prob_threshold was {:?}, expected Some(<= -0.8) to reject \
             low-confidence hallucinations",
            request.log_prob_threshold
        );
    }

    #[test]
    fn fw_fast_request_disables_prompt_for_speed() {
        let request = worker_request_for(
            "small.en",
            DecodeMode::Fast,
            FasterWhisperRequestOverrides::default(),
            "AQID".to_string(),
        );
        assert!(!request.condition_on_previous_text);
        assert!(request.initial_prompt.is_none());
        assert!(request.allow_backend_fallback);
        assert!(!request.backend_preference.is_empty());
        assert_eq!(
            request.without_timestamps,
            super::fw_without_timestamps_enabled()
        );
    }

    #[test]
    fn fw_overrides_replace_request_defaults() {
        let request = worker_request_for(
            "small.en",
            DecodeMode::Balanced,
            FasterWhisperRequestOverrides {
                beam_size: Some(5),
                best_of: Some(3),
                initial_prompt: Some(" spell uncommon words literally ".to_string()),
                condition_on_previous_text: Some(true),
                vad_filter: Some(true),
                without_timestamps: Some(false),
                temperature: Some(0.0),
                no_speech_threshold: Some(0.5),
                log_prob_threshold: Some(-1.2),
                compression_ratio_threshold: Some(2.1),
            },
            "AQID".to_string(),
        );
        assert_eq!(request.beam_size, 5);
        assert_eq!(request.best_of, 3);
        assert_eq!(
            request.initial_prompt.as_deref(),
            Some("spell uncommon words literally")
        );
        assert!(request.condition_on_previous_text);
        assert!(request.vad_filter);
        assert!(!request.without_timestamps);
        assert_eq!(request.temperature, Some(0.0));
        assert_eq!(request.no_speech_threshold, Some(0.5));
        assert_eq!(request.log_prob_threshold, Some(-1.2));
        assert_eq!(request.compression_ratio_threshold, Some(2.1));
    }

    #[test]
    fn pcm_encoding_produces_non_empty_base64_payload() {
        let encoded = encode_pcm16_base64(&[0.0_f32, 0.25_f32, -0.25_f32]);
        assert!(!encoded.is_empty());
    }

    #[test]
    fn backend_policy_prefers_auto_for_small_model_when_gpu_is_allowed() {
        let backend = select_worker_backend_preference("small.en", false, false, false, true);
        assert_eq!(backend, "auto");
    }

    #[test]
    fn backend_policy_keeps_tiny_and_base_on_cpu_by_default() {
        let tiny = select_worker_backend_preference("tiny.en", false, false, false, true);
        let base = select_worker_backend_preference("base.en", false, false, false, true);
        assert_eq!(tiny, "cpu");
        assert_eq!(base, "cpu");
    }

    #[test]
    fn backend_policy_honors_force_flags_in_safe_order() {
        let force_gpu = select_worker_backend_preference("small.en", false, true, true, false);
        assert_eq!(force_gpu, "cuda");

        let force_cpu = select_worker_backend_preference("small.en", true, true, false, true);
        assert_eq!(force_cpu, "cpu");
    }

    #[test]
    fn backend_policy_falls_back_to_cpu_when_gpu_is_persistently_disabled_or_auto_off() {
        let persistent_lock =
            select_worker_backend_preference("small.en", false, false, true, true);
        let auto_disabled =
            select_worker_backend_preference("small.en", false, false, false, false);
        assert_eq!(persistent_lock, "cpu");
        assert_eq!(auto_disabled, "cpu");
    }
}

fn next_request_id() -> u64 {
    let slot = WORKER.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = slot.lock() {
        if let Some(worker) = guard.as_mut() {
            let id = worker.next_id;
            worker.next_id = worker.next_id.saturating_add(1);
            return id;
        }
    }
    now_utc_ms()
}

fn hf_home_dir() -> PathBuf {
    if let Ok(path) = std::env::var("VOICEWAVE_FASTER_WHISPER_CACHE_DIR") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    if let Some(proj_dirs) = ProjectDirs::from("com", "voicewave", "localcore") {
        return proj_dirs.data_dir().join("faster-whisper-cache");
    }
    std::env::temp_dir().join("voicewave-faster-whisper-cache")
}

fn now_utc_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}
