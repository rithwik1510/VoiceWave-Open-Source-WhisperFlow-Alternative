//! On-device LLM "polish" worker client (plan 005, Phase 3).
//!
//! Mirrors the STRUCTURE of `inference/faster_whisper.rs` (a long-lived Python
//! child process spoken to over a stdin/stdout JSON-line protocol) but with a
//! COMPLETELY SEPARATE static handle + request gate so it can never contend
//! with the ASR worker. This path is off by default; it only runs when the
//! `llm_polish_enabled` setting is on, and only in a background task spawned
//! AFTER a successful insertion. It is best-effort: any error yields
//! `Ok(None)` and the caller silently keeps the deterministic text.

use super::InferenceError;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read, Write};
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{
    mpsc::{self, RecvTimeoutError},
    Mutex, OnceLock,
};
use std::time::Duration;

/// Set while a polish-model download is running so repeated enable toggles
/// cannot start overlapping downloads that would clobber the same `.partial`.
static POLISH_DOWNLOAD_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

/// True while the polish model is actively downloading.
pub fn is_polish_model_downloading() -> bool {
    POLISH_DOWNLOAD_IN_PROGRESS.load(Ordering::Relaxed)
}

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
#[cfg(target_os = "windows")]
const DETACHED_PROCESS: u32 = 0x0000_0008;

/// Repo-relative dev path to the prototype 1.5B GGUF. Production distribution
/// (a signed model in the catalog) is a follow-up; do NOT hardcode an absolute
/// user path here.
const DEV_MODEL_REL_PATH: &str =
    "scripts/llm-polish/models/Qwen2.5-1.5B-Instruct-Q4_K_M.gguf";

/// The polish model file name, shared by the dev path and the shipped
/// download. Kept in sync with DEV_MODEL_REL_PATH's tail.
pub const POLISH_MODEL_FILENAME: &str = "Qwen2.5-1.5B-Instruct-Q4_K_M.gguf";

/// Public HuggingFace URL for the shipped polish model (~1.02 GB). Downloaded
/// on demand to the app data dir the first time the user enables AI polish, so
/// the installer stays lean and only opt-in users pay the download.
const POLISH_MODEL_URL: &str =
    "https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct-GGUF/resolve/main/qwen2.5-1.5b-instruct-q4_k_m.gguf";

/// Floor on the finished file size (bytes). The real file is ~1.02 GB; anything
/// well under this is a truncated/partial download and must not be treated as
/// installed. Guards against a half-download being loaded as a valid GGUF.
const POLISH_MODEL_MIN_BYTES: u64 = 900_000_000;

/// Directory where the shipped polish model is downloaded: the app data
/// `models` dir (same root faster-whisper models use). Returns None only if the
/// platform data dir cannot be resolved.
fn polish_model_store_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("com", "voicewave", "localcore")
        .map(|dirs| dirs.data_dir().join("models"))
}

/// Full path to where the shipped polish model lives once downloaded.
fn polish_model_store_path() -> Option<PathBuf> {
    polish_model_store_dir().map(|dir| dir.join(POLISH_MODEL_FILENAME))
}

/// True if the polish model is fully downloaded and usable (present and at
/// least the expected size). Cheap; safe to call from a status command.
pub fn is_polish_model_present() -> bool {
    resolve_model_path().is_some()
}

/// Generous ceiling for a single polish round-trip. The FIRST polish also pays
/// the lazy GGUF load inside the worker, so this is deliberately larger than a
/// warm decode would need; the whole path runs in a background task off the hot
/// path, so a long ceiling costs nothing in user-facing latency.
const POLISH_RESPONSE_TIMEOUT_MS_DEFAULT: u64 = 60_000;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkerRequest {
    id: u64,
    command: String,
    text: String,
    /// Polish profile prompt id (plan 010): "standard" | "coding" |
    /// "writing" | "casual" | "literal". Workers predating profiles ignore
    /// unknown JSON fields, so this is back-compatible.
    profile: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkerResponse {
    id: Option<u64>,
    ok: bool,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    text: Option<String>,
}

struct PolishWorkerProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: Option<BufReader<ChildStdout>>,
    next_id: u64,
}

/// SEPARATE from the ASR worker on purpose. Never reuse the ASR `WORKER`.
static POLISH_WORKER: OnceLock<Mutex<Option<PolishWorkerProcess>>> = OnceLock::new();
/// SEPARATE from `WORKER_REQUEST_GATE`. Serializes polish requests only.
static POLISH_GATE: Mutex<()> = Mutex::new(());

/// Best-effort polish of a raw transcript with the default (Standard)
/// profile prompt. See [`polish_text_for_profile`].
pub async fn polish_text(raw: String) -> Result<Option<String>, InferenceError> {
    polish_text_for_profile(raw, "standard".to_string()).await
}

/// Best-effort polish of a raw transcript under a specific profile prompt
/// (plan 010). Returns:
/// - `Ok(Some(polished))` when the worker returned a rewrite,
/// - `Ok(None)` on ANY failure (missing model, worker error, timeout, empty),
///
/// so the caller can always fall back to the deterministic text.
pub async fn polish_text_for_profile(
    raw: String,
    profile: String,
) -> Result<Option<String>, InferenceError> {
    let result = tokio::task::spawn_blocking(move || polish_text_blocking(raw, profile))
        .await
        .map_err(|err| InferenceError::RuntimeJoin(format!("polish worker join failure: {err}")))?;
    Ok(result)
}

fn polish_text_blocking(raw: String, profile: String) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Serialize polish requests through our OWN gate (never the ASR gate).
    let _gate = POLISH_GATE.lock().ok();
    let request = WorkerRequest {
        id: next_request_id(),
        command: "polish".to_string(),
        text: raw.clone(),
        profile,
    };
    match send_polish_request_inner(request) {
        Ok(response) if response.ok => response
            .text
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        Ok(response) => {
            if let Some(reason) = response.error {
                eprintln!("voicewave: polish worker rejected request: {reason}");
            }
            None
        }
        Err(err) => {
            eprintln!("voicewave: polish worker error: {err}");
            None
        }
    }
}

fn send_polish_request_inner(request: WorkerRequest) -> Result<WorkerResponse, InferenceError> {
    let mut worker = checkout_worker()?;

    let payload = serde_json::to_string(&request).map_err(|err| {
        InferenceError::RuntimeJoin(format!("encode polish request failed: {err}"))
    })?;
    if let Err(err) = write_request(&mut worker, &payload) {
        let _ = worker.child.kill();
        let _ = worker.child.wait();
        return Err(err);
    }

    let line = match read_response_line_with_timeout(&mut worker, response_timeout()) {
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
                "parse polish response failed: {err}"
            )));
        }
    };

    // On an id mismatch the worker is out of sync; kill + respawn next time
    // (mirrors the ASR client's defensive behavior).
    if response.id != Some(request.id) {
        let _ = worker.child.kill();
        let _ = worker.child.wait();
        return Err(InferenceError::RuntimeJoin(format!(
            "polish response id mismatch (expected {}, got {:?})",
            request.id, response.id
        )));
    }
    checkin_worker(worker);
    Ok(response)
}

fn write_request(worker: &mut PolishWorkerProcess, payload: &str) -> Result<(), InferenceError> {
    worker
        .stdin
        .write_all(payload.as_bytes())
        .map_err(|err| InferenceError::RuntimeJoin(format!("polish stdin write failed: {err}")))?;
    worker.stdin.write_all(b"\n").map_err(|err| {
        InferenceError::RuntimeJoin(format!("polish stdin newline write failed: {err}"))
    })?;
    worker
        .stdin
        .flush()
        .map_err(|err| InferenceError::RuntimeJoin(format!("polish stdin flush failed: {err}")))?;
    Ok(())
}

fn checkout_worker() -> Result<PolishWorkerProcess, InferenceError> {
    let slot = POLISH_WORKER.get_or_init(|| Mutex::new(None));
    let mut guard = slot
        .lock()
        .map_err(|_| InferenceError::RuntimeJoin("polish worker lock poisoned".to_string()))?;
    let needs_spawn = match guard.as_mut() {
        Some(worker) => worker.child.try_wait().ok().flatten().is_some(),
        None => true,
    };
    if needs_spawn {
        *guard = Some(spawn_polish_worker()?);
    }
    guard
        .take()
        .ok_or_else(|| InferenceError::RuntimeJoin("failed to acquire polish worker".to_string()))
}

fn checkin_worker(mut worker: PolishWorkerProcess) {
    let slot = POLISH_WORKER.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = slot.lock() {
        if guard.is_none() {
            *guard = Some(worker);
            return;
        }
    }
    let _ = worker.child.kill();
    let _ = worker.child.wait();
}

fn response_timeout() -> Duration {
    let ms = std::env::var("VOICEWAVE_POLISH_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(POLISH_RESPONSE_TIMEOUT_MS_DEFAULT);
    Duration::from_millis(ms)
}

fn read_response_line_with_timeout(
    worker: &mut PolishWorkerProcess,
    timeout: Duration,
) -> Result<String, InferenceError> {
    let stdout = worker
        .stdout
        .take()
        .ok_or_else(|| InferenceError::RuntimeJoin("polish stdout unavailable".to_string()))?;
    let (tx, rx) = mpsc::channel();

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
                    "polish worker exited unexpectedly".to_string(),
                ));
            }
            Ok(line)
        }
        Ok(Err((stdout, err))) => {
            worker.stdout = Some(stdout);
            Err(InferenceError::RuntimeJoin(format!(
                "polish stdout read failed: {err}"
            )))
        }
        Err(RecvTimeoutError::Timeout) => {
            let _ = worker.child.kill();
            let _ = worker.child.wait();
            Err(InferenceError::RuntimeJoin(format!(
                "polish worker timed out after {} ms",
                timeout.as_millis()
            )))
        }
        Err(RecvTimeoutError::Disconnected) => {
            let _ = worker.child.kill();
            let _ = worker.child.wait();
            Err(InferenceError::RuntimeJoin(
                "polish worker response channel disconnected".to_string(),
            ))
        }
    }
}

fn spawn_polish_worker() -> Result<PolishWorkerProcess, InferenceError> {
    let worker_path = resolve_worker_path()?;
    let python = resolve_python_path()?;
    let model_path = resolve_model_path();

    let mut command = Command::new(&python);
    command
        .arg(worker_path.as_os_str())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        // Discard the worker's stderr: results and errors come back as JSON on
        // stdout, so stderr is only llama.cpp's internal (esp. CUDA) logging.
        // Piping it without draining could fill the pipe buffer and deadlock the
        // worker under the chattier GPU build.
        .stderr(Stdio::null());
    if let Some(model_path) = model_path.as_ref() {
        command.env("VOICEWAVE_POLISH_MODEL_PATH", model_path);
    }
    #[cfg(target_os = "windows")]
    {
        command.creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS);
    }

    let mut child = command.spawn().map_err(|err| {
        InferenceError::RuntimeJoin(format!(
            "failed to spawn polish worker using '{python}': {err}"
        ))
    })?;

    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| InferenceError::RuntimeJoin("polish stdin unavailable".to_string()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| InferenceError::RuntimeJoin("polish stdout unavailable".to_string()))?;
    let mut stdout_reader = BufReader::new(stdout);

    let mut ready_line = String::new();
    let ready_bytes = stdout_reader.read_line(&mut ready_line).map_err(|err| {
        InferenceError::RuntimeJoin(format!("polish worker ready read failed: {err}"))
    })?;
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
        return Err(InferenceError::RuntimeJoin(format!(
            "polish worker exited before ready. stderr: {}",
            stderr.trim()
        )));
    }

    Ok(PolishWorkerProcess {
        child,
        stdin,
        stdout: Some(stdout_reader),
        next_id: 1,
    })
}

fn next_request_id() -> u64 {
    let slot = POLISH_WORKER.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = slot.lock() {
        if let Some(worker) = guard.as_mut() {
            let id = worker.next_id;
            worker.next_id = worker.next_id.saturating_add(1);
            return id;
        }
    }
    1
}

fn resolve_worker_path() -> Result<PathBuf, InferenceError> {
    if let Ok(path) = std::env::var("VOICEWAVE_POLISH_WORKER") {
        let value = PathBuf::from(path.trim());
        if value.exists() {
            return Ok(value);
        }
    }
    for candidate in worker_path_candidates() {
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(InferenceError::RuntimeJoin(
        "polish worker script is missing from this install.".to_string(),
    ))
}

fn worker_path_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(exe_dir) = current_exe_dir() {
        candidates.push(exe_dir.join("llm-polish").join("polish_worker.py"));
        candidates.push(
            exe_dir
                .join("resources")
                .join("llm-polish")
                .join("polish_worker.py"),
        );
    }
    // Dev tree: CARGO_MANIFEST_DIR is src-tauri.
    candidates.push(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("windows")
            .join("llm-polish")
            .join("polish_worker.py"),
    );
    candidates
}

/// Minimal duplicate of the venv python resolution in `faster_whisper.rs`
/// (its helpers are private and that file is out of scope). Reuses the same
/// `VOICEWAVE_FASTER_WHISPER_PYTHON` override and the `.venv-faster-whisper`
/// layout so the polish worker runs on the exact interpreter that already has
/// `llama_cpp` installed.
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
        "polish python runtime is missing from this install.".to_string(),
    ))
}

fn python_path_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(exe_dir) = current_exe_dir() {
        candidates.extend(python_layout_candidates(&exe_dir.join("faster-whisper")));
        candidates.extend(python_layout_candidates(
            &exe_dir.join("resources").join("faster-whisper"),
        ));
    }
    candidates.extend(python_layout_candidates(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(".venv-faster-whisper"),
    ));
    candidates
}

fn python_layout_candidates(root: &Path) -> Vec<PathBuf> {
    vec![
        root.join("Scripts").join("python.exe"),
        root.join("python.exe"),
        root.join("python").join("Scripts").join("python.exe"),
        root.join("python").join("python.exe"),
    ]
}

/// Resolve the polish model GGUF path. Order: (1) explicit
/// `VOICEWAVE_POLISH_MODEL_PATH` env; (2) a repo-relative dev path resolved from
/// CWD, then from the exe dir's ancestors. Returns `None` when nothing exists —
/// the worker then reports "model not found" and `polish_text` no-ops.
fn resolve_model_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("VOICEWAVE_POLISH_MODEL_PATH") {
        let candidate = PathBuf::from(path.trim());
        if candidate.exists() {
            return Some(candidate);
        }
    }

    // Shipped location: the model downloaded on first enable. Require the full
    // size so a partial/aborted download is never loaded as a valid GGUF.
    if let Some(store) = polish_model_store_path() {
        if let Ok(meta) = std::fs::metadata(&store) {
            if meta.len() >= POLISH_MODEL_MIN_BYTES {
                return Some(store);
            }
        }
    }

    let mut roots: Vec<PathBuf> = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        roots.push(cwd);
    }
    // CARGO_MANIFEST_DIR is src-tauri; the repo root is its parent.
    roots.push(Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf());
    roots.push(Path::new(env!("CARGO_MANIFEST_DIR")).join(".."));
    if let Some(exe_dir) = current_exe_dir() {
        roots.push(exe_dir);
    }

    for root in roots {
        let candidate = root.join(DEV_MODEL_REL_PATH);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

/// Download the polish model to the app data store, reporting progress via
/// `on_progress(downloaded_bytes, total_bytes)`. Streams to a `.partial` file
/// and renames on success, so an interrupted download never leaves a truncated
/// file that `resolve_model_path` would treat as installed. Idempotent: returns
/// immediately if the model is already present.
pub fn download_polish_model<F: FnMut(u64, u64)>(
    mut on_progress: F,
) -> Result<PathBuf, String> {
    if let Some(existing) = resolve_model_path() {
        return Ok(existing);
    }
    // Claim the single-download slot; bail if another download is already
    // running. The guard resets on every return path below.
    if POLISH_DOWNLOAD_IN_PROGRESS
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("a polish model download is already in progress".to_string());
    }
    struct DownloadGuard;
    impl Drop for DownloadGuard {
        fn drop(&mut self) {
            POLISH_DOWNLOAD_IN_PROGRESS.store(false, Ordering::SeqCst);
        }
    }
    let _guard = DownloadGuard;

    let store_dir =
        polish_model_store_dir().ok_or_else(|| "cannot resolve app data directory".to_string())?;
    std::fs::create_dir_all(&store_dir)
        .map_err(|err| format!("failed to create model directory: {err}"))?;
    let final_path = store_dir.join(POLISH_MODEL_FILENAME);
    let partial_path = store_dir.join(format!("{POLISH_MODEL_FILENAME}.partial"));

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(30))
        .timeout_read(Duration::from_secs(120))
        .build();
    let response = agent
        .get(POLISH_MODEL_URL)
        .call()
        .map_err(|err| format!("model download request failed: {err}"))?;
    let total: u64 = response
        .header("Content-Length")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);

    let mut reader = response.into_reader();
    let mut file = std::fs::File::create(&partial_path)
        .map_err(|err| format!("failed to create partial file: {err}"))?;
    let mut buffer = vec![0u8; 256 * 1024];
    let mut downloaded: u64 = 0;
    let mut last_report: u64 = 0;
    on_progress(0, total);
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|err| format!("download read error: {err}"))?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])
            .map_err(|err| format!("download write error: {err}"))?;
        downloaded += read as u64;
        // Throttle progress to ~every 4 MB so we don't flood the UI channel.
        if downloaded - last_report >= 4 * 1024 * 1024 {
            last_report = downloaded;
            on_progress(downloaded, total);
        }
    }
    file.flush()
        .map_err(|err| format!("download flush error: {err}"))?;
    drop(file);

    if downloaded < POLISH_MODEL_MIN_BYTES {
        let _ = std::fs::remove_file(&partial_path);
        return Err(format!(
            "downloaded model is truncated ({downloaded} bytes, expected >= {POLISH_MODEL_MIN_BYTES})"
        ));
    }
    std::fs::rename(&partial_path, &final_path)
        .map_err(|err| format!("failed to finalize model file: {err}"))?;
    on_progress(downloaded, total.max(downloaded));
    Ok(final_path)
}

fn current_exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
}
