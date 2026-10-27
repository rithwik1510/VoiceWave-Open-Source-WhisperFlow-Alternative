use crate::atomic_file;
use crate::inference::gpu_select::{
    model_tier, CpuSupport, HardwareProfile, ModelTier, PARAKEET_MODEL_ID,
};
use crate::inference::AsrRung;
use base64::Engine;
use directories::ProjectDirs;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    env, fs,
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

mod net;
pub use net::DownloadFailureStage;

const FASTER_WHISPER_FORMAT: &str = "faster-whisper";
const FASTER_WHISPER_VERSION: &str = "faster-whisper-v1";
const WCPP_FORMAT: &str = "ggml";
/// huggingface.co/ggerganov/whisper.cpp commit the GGML models are pinned to.
/// The revision is part of each download URL, so the SHA-256 values below
/// stay valid even if the repo's files are replaced later.
const GGML_REPO_REVISION: &str = "5359861c739e955e79d9a303bcbc70fb988958b1";
const WCPP_VERSION: &str = "wcpp-5359861";
const MANIFEST_VERIFY_KEY_B64: &str = "BFHdCqQYd/56J4tm8cYMUnhrfHOR6YyJqiwXPgO+5gU=";
// Generated via `generate_fw_small_catalog_signature` test (dev key = production key).
const BUILTIN_FW_SMALL_SIGNATURE: &str =
    "6pLuyp8Z495RkvKNs9MSnDu8DvG6i+kd8YHtWfQReMKNAB8DVrN6fa9I695K7R+XvxnumprISe1cOQS089cKAA==";
// Generated via `generate_fw_turbo_catalog_signature` test (dev key = production key).
const BUILTIN_FW_LARGE_TURBO_SIGNATURE: &str =
    "IfGO/sqcNE4flIvVG/YcTlrx2DbUDntDGHUtG1dWTiiwEwqwZbYZO/pCwf0q8YZ0gMRmHYyQLddf3Es+Ux42DQ==";
// Signed with the same dev key over the rows in `build_whispercpp_catalog`
// (payload: `manifest_signature_payload`). Re-sign if any field changes.
const BUILTIN_WCPP_SMALL_EN_SIGNATURE: &str =
    "R7gA3H6JW+GI+vJamDV53bLHKTS4/WMpyLphTSvAjjZIkyt1IYC/x61Uzc+MGo8QHgVA+b663bP6j8UHFvY4Dw==";
const BUILTIN_WCPP_LARGE_TURBO_SIGNATURE: &str =
    "501QKU4taYPMqEXngLpXgowESdwCbDfZK5WQljaNXIsB0gIoy6plf/5CFp7tzGxAsr4L5XTnboz0rGC5918QCw==";
/// huggingface.co/ggml-org/parakeet-GGUF commit the Parakeet model is pinned to.
const PARAKEET_REPO_REVISION: &str = "35156454d1a39de06863303dd209fd2bed6ee079";
// Same dev key and payload as the rows above; re-sign if any field changes.
const BUILTIN_WCPP_PARAKEET_V3_SIGNATURE: &str =
    "F5+YJ5jUKBTVItEwWLSWS1p+C68OrftCP24JKSrkve9b+2Wjuu2RSiEdiEhmgJdrenE5VNXMBsI2wy+dHaCACw==";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SignedModelManifest {
    pub model_id: String,
    pub version: String,
    #[serde(default = "default_model_format")]
    pub format: String,
    pub size: u64,
    pub sha256: String,
    pub license: String,
    pub download_url: String,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelCatalogItem {
    pub model_id: String,
    pub display_name: String,
    pub version: String,
    #[serde(default = "default_model_format")]
    pub format: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub license: String,
    pub download_url: String,
    pub signature: String,
    /// The model VoiceWave recommends for THIS machine's hardware (exactly one
    /// catalog row is true). See `HardwareProfile::recommended_model_id`.
    #[serde(default)]
    pub recommended: bool,
    /// One honest sentence on how the model will perform on this machine.
    #[serde(default)]
    pub hardware_note: String,
    /// "Small", "Large Turbo" or Parakeet, whatever the family.
    pub tier: ModelTier,
    /// One of the rows setup offers on this PC: Parakeet where the speech
    /// workers run, plus Small and Large Turbo (the GGML family on GPU-path
    /// machines, faster-whisper elsewhere). False for an installed model of
    /// the other family, listed only so it can be used or removed.
    #[serde(default)]
    pub primary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstalledModel {
    pub model_id: String,
    pub version: String,
    #[serde(default = "default_model_format")]
    pub format: String,
    pub size_bytes: u64,
    pub file_path: String,
    pub sha256: String,
    pub installed_at_utc_ms: u64,
    pub checksum_verified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ModelStatusState {
    Idle,
    Downloading,
    Paused,
    Installed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    pub model_id: String,
    pub state: ModelStatusState,
    pub progress: u8,
    pub active: bool,
    pub installed: bool,
    pub message: Option<String>,
    pub installed_model: Option<InstalledModel>,
    pub downloaded_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    pub resumable: bool,
    /// Set only on a failed whisper.cpp/Parakeet download.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_stage: Option<DownloadFailureStage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelDownloadRequest {
    pub model_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelEvent {
    pub model_id: String,
    pub state: ModelStatusState,
    pub progress: u8,
    pub message: Option<String>,
    pub downloaded_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_stage: Option<DownloadFailureStage>,
}

impl ModelEvent {
    pub fn from_status(status: &ModelStatus) -> Self {
        Self {
            model_id: status.model_id.clone(),
            state: status.state.clone(),
            progress: status.progress,
            message: status.message.clone(),
            downloaded_bytes: status.downloaded_bytes,
            total_bytes: status.total_bytes,
            failure_stage: status.failure_stage,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("unknown model id: {0}")]
    UnknownModel(String),
    #[error("failed to read model data: {0}")]
    Read(std::io::Error),
    #[error("failed to write model data: {0}")]
    Write(std::io::Error),
    /// A failed HTTP request or response body; see `net::classify_*`.
    #[error("model download failed ({stage}): {detail}")]
    Network {
        stage: DownloadFailureStage,
        http_status: Option<u16>,
        retryable: bool,
        detail: String,
    },
    #[error("couldn't move the verified model into place: {0}")]
    Rename(std::io::Error),
    #[error("failed to parse model metadata: {0}")]
    Parse(serde_json::Error),
    #[error("cannot resolve app data directory")]
    AppData,
    #[error("manifest signature verification failed for {model_id}")]
    ManifestSignatureInvalid { model_id: String },
    #[error("manifest metadata mismatch for {model_id}: expected size {expected}, got {actual}")]
    ManifestSizeMismatch {
        model_id: String,
        expected: u64,
        actual: u64,
    },
    #[error("checksum mismatch for {model_id}: expected {expected}, got {actual}")]
    ChecksumMismatch {
        model_id: String,
        expected: String,
        actual: String,
        quarantine_path: Option<String>,
    },
    #[error("download interrupted for {model_id}: source ended before expected size {expected}")]
    CorruptSource { model_id: String, expected: u64 },
    #[error("low disk while installing {model_id}; needed bytes would exceed {limit}")]
    LowDisk { model_id: String, limit: u64 },
    #[error("unsupported download url for {model_id}: {url}")]
    UnsupportedDownloadUrl { model_id: String, url: String },
    #[error("unsupported model format for {model_id}: {format} (expected gguf or bin)")]
    UnsupportedModelFormat { model_id: String, format: String },
    #[error("model source file for {model_id} was not found at {expected_path}; provide a model artifact in VOICEWAVE_MODEL_SOURCE_DIR or app data model-sources")]
    SourceMissing {
        model_id: String,
        expected_path: String,
    },
    #[error("downloaded files for {model_id} are incomplete: {reason}")]
    IncompleteModel { model_id: String, reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct InstalledModelStore {
    installed: Vec<InstalledModel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DownloadCheckpoint {
    model_id: String,
    version: String,
    partial_path: String,
    downloaded_bytes: u64,
    total_bytes: u64,
    state: ModelStatusState,
    last_error: Option<String>,
    #[serde(default)]
    failure_stage: Option<DownloadFailureStage>,
    updated_at_utc_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct DownloadCheckpointStore {
    downloads: Vec<DownloadCheckpoint>,
}

enum DownloadSource {
    Http(String),
    File(PathBuf),
}

/// A download prepared under the `model_manager` lock by
/// `ModelManager::begin_download`, run without it (`run`), then recorded
/// under the lock again by `ModelManager::finish_download`.
pub struct DownloadPlan {
    manifest: SignedModelManifest,
    checkpoint: DownloadCheckpoint,
    source: DownloadSource,
    quarantine_dir: PathBuf,
    max_storage_bytes: Option<u64>,
    chunk_bytes: usize,
}

/// How `DownloadPlan::run` ended when it did not fail.
pub enum DownloadOutcome {
    /// The partial file is complete and matches the manifest's SHA-256.
    Verified { sha256: String },
    /// Cancelled or paused; the partial file is kept for resume.
    Stopped,
}

/// Progress events are throttled: HTTP reads return a few KB at a time.
const PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(250);
/// Waits before each automatic resume after a transient failure; its length
/// is the retry budget (refilled whenever an attempt gets further than any
/// before it). The
/// long tail (about two minutes in all) covers a laptop waking from sleep,
/// where Wi-Fi can take 20 s or more to come back.
const RETRY_BACKOFF: [Duration; 5] = [
    Duration::from_secs(2),
    Duration::from_secs(5),
    Duration::from_secs(10),
    Duration::from_secs(30),
    Duration::from_secs(60),
];

impl DownloadPlan {
    /// Downloads the rest of the artifact into the partial file and checks it
    /// against the pinned SHA-256. Touches no installed model and no manager
    /// state, so it runs without the `model_manager` lock.
    ///
    /// Resume is always on. A transient network failure (see
    /// `ModelError::is_transient`) is retried after a short wait, resuming
    /// from the bytes on disk; the budget refills whenever an attempt gets
    /// further than any before it, so only a connection that stays down (or
    /// never gets past where it was) gives up. If a resumed
    /// download fails the hash, the kept bytes were bad (or the server's file
    /// changed), so the partial is discarded and the download restarts from
    /// zero once; a download that still mismatches is quarantined and
    /// reported.
    pub fn run<F, C, P>(
        &mut self,
        mut should_cancel: C,
        mut should_pause: P,
        mut on_progress: F,
    ) -> Result<DownloadOutcome, ModelError>
    where
        F: FnMut(ModelStatus),
        C: FnMut() -> bool,
        P: FnMut() -> bool,
    {
        let model_id = self.manifest.model_id.clone();
        let started = Instant::now();
        let start_offset = self.checkpoint.downloaded_bytes;
        let total = self.checkpoint.total_bytes;
        if start_offset > 0 {
            tracing::info!("{model_id}: resuming download at {start_offset} of {total} bytes");
        } else {
            tracing::info!("{model_id}: starting download of {total} bytes");
        }

        let mut retries = 0_u32;
        let result = self.run_attempts(
            &mut retries,
            &mut should_cancel,
            &mut should_pause,
            &mut on_progress,
        );
        let done = self.checkpoint.downloaded_bytes;
        match &result {
            Ok(DownloadOutcome::Verified { .. }) => tracing::info!(
                "{model_id}: downloaded and verified {total} bytes in {:.1} s (resumed at {start_offset}, {retries} retries)",
                started.elapsed().as_secs_f32()
            ),
            Ok(DownloadOutcome::Stopped) => tracing::info!(
                "{model_id}: download {:?} at {done} of {total} bytes",
                self.checkpoint.state
            ),
            Err(err) => tracing::warn!(
                "{model_id}: download failed (stage {}) at {done} of {total} bytes on attempt {}: {err}",
                err.failure_stage(),
                retries + 1
            ),
        }
        result
    }

    fn run_attempts<F, C, P>(
        &mut self,
        retries: &mut u32,
        should_cancel: &mut C,
        should_pause: &mut P,
        on_progress: &mut F,
    ) -> Result<DownloadOutcome, ModelError>
    where
        F: FnMut(ModelStatus),
        C: FnMut() -> bool,
        P: FnMut() -> bool,
    {
        let mut failed_in_a_row = 0_usize;
        let mut restarted_from_zero = false;
        // Furthest byte any attempt has reached. Only passing it refills the
        // retry budget: a server that ignores Range restarts each attempt from
        // zero, and comparing against the previous attempt alone would let
        // alternating short and long attempts retry forever.
        let mut furthest = self.checkpoint.downloaded_bytes;
        loop {
            let resumed_from = self.checkpoint.downloaded_bytes;
            let actual = match self.fetch(should_cancel, should_pause, on_progress) {
                Ok(Some(actual)) => actual,
                Ok(None) => return Ok(DownloadOutcome::Stopped),
                Err(err) if err.is_transient() => {
                    if self.checkpoint.downloaded_bytes > furthest {
                        furthest = self.checkpoint.downloaded_bytes;
                        failed_in_a_row = 0;
                    }
                    let Some(&delay) = RETRY_BACKOFF.get(failed_in_a_row) else {
                        return Err(err);
                    };
                    failed_in_a_row += 1;
                    *retries += 1;
                    tracing::warn!(
                        "{}: download interrupted (stage {}) at {} of {} bytes on attempt {}; retry {failed_in_a_row} of {} in {} s: {err}",
                        self.manifest.model_id,
                        err.failure_stage(),
                        self.checkpoint.downloaded_bytes,
                        self.checkpoint.total_bytes,
                        *retries,
                        RETRY_BACKOFF.len(),
                        delay.as_secs()
                    );
                    self.checkpoint.state = ModelStatusState::Downloading;
                    self.checkpoint.last_error = Some(format!(
                        "Connection problem, retrying ({failed_in_a_row} of {})…",
                        RETRY_BACKOFF.len()
                    ));
                    self.checkpoint.updated_at_utc_ms = now_utc_ms();
                    on_progress(status_from_checkpoint(&self.checkpoint, false));
                    if self.wait_before_retry(delay, should_cancel, should_pause) {
                        on_progress(status_from_checkpoint(&self.checkpoint, false));
                        return Ok(DownloadOutcome::Stopped);
                    }
                    continue;
                }
                Err(err) => return Err(err),
            };
            if actual == self.manifest.sha256 {
                return Ok(DownloadOutcome::Verified { sha256: actual });
            }

            let model_id = self.manifest.model_id.clone();
            let partial_path = PathBuf::from(&self.checkpoint.partial_path);
            self.checkpoint.downloaded_bytes = 0;
            self.checkpoint.updated_at_utc_ms = now_utc_ms();
            if resumed_from > 0 && !restarted_from_zero {
                restarted_from_zero = true;
                tracing::warn!(
                    "{model_id}: resumed download failed the checksum; restarting from zero"
                );
                fs::remove_file(&partial_path).map_err(ModelError::Write)?;
                furthest = 0;
                self.checkpoint.last_error = Some(
                    "Saved download data was corrupt; restarting from the beginning.".to_string(),
                );
                on_progress(status_from_checkpoint(&self.checkpoint, false));
                continue;
            }

            let quarantine_path = quarantine_file(
                &self.quarantine_dir,
                &partial_path,
                &model_id,
                "checksum-mismatch",
            )?
            .map(|path| path.to_string_lossy().to_string());
            return Err(ModelError::ChecksumMismatch {
                model_id,
                expected: self.manifest.sha256.clone(),
                actual,
                quarantine_path,
            });
        }
    }

    /// Waits out a retry backoff in short slices so cancel and pause still
    /// land promptly. True when one did (recorded in the checkpoint).
    fn wait_before_retry<C, P>(
        &mut self,
        delay: Duration,
        should_cancel: &mut C,
        should_pause: &mut P,
    ) -> bool
    where
        C: FnMut() -> bool,
        P: FnMut() -> bool,
    {
        let deadline = Instant::now() + delay;
        loop {
            if self.stop_requested(should_cancel, should_pause) {
                return true;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            std::thread::sleep(left.min(Duration::from_millis(200)));
        }
    }

    /// One pass: hash the bytes already on disk (streamed, never loaded into
    /// RAM), then download and hash the rest. Returns the SHA-256 of the
    /// complete partial file, or None when cancelled or paused.
    fn fetch<F, C, P>(
        &mut self,
        should_cancel: &mut C,
        should_pause: &mut P,
        on_progress: &mut F,
    ) -> Result<Option<String>, ModelError>
    where
        F: FnMut(ModelStatus),
        C: FnMut() -> bool,
        P: FnMut() -> bool,
    {
        let partial_path = PathBuf::from(&self.checkpoint.partial_path);
        // A plain write handle, not append: on Windows an append-only handle
        // cannot truncate, which a restart from zero needs.
        let mut partial_file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&partial_path)
            .map_err(ModelError::Write)?;
        partial_file
            .set_len(self.checkpoint.downloaded_bytes)
            .map_err(ModelError::Write)?;
        partial_file
            .seek(SeekFrom::End(0))
            .map_err(ModelError::Write)?;

        let mut hasher = Sha256::new();
        if self.checkpoint.downloaded_bytes > 0 {
            // Failing to read our own partial file is a file-access problem
            // (usually antivirus holding it), reported like a write failure.
            let kept = fs::File::open(&partial_path).map_err(ModelError::Write)?;
            io::copy(
                &mut kept.take(self.checkpoint.downloaded_bytes),
                &mut hasher,
            )
            .map_err(ModelError::Write)?;
        }

        let mut source: Box<dyn Read> =
            if self.checkpoint.downloaded_bytes >= self.checkpoint.total_bytes {
                // Already complete (e.g. a verified file whose move failed).
                Box::new(io::empty())
            } else {
                self.open_source(&mut partial_file, &mut hasher)?
            };

        let mut buffer = vec![0_u8; self.chunk_bytes];
        let mut last_emit: Option<Instant> = None;
        while self.checkpoint.downloaded_bytes < self.checkpoint.total_bytes {
            if self.stop_requested(should_cancel, should_pause) {
                partial_file.flush().map_err(ModelError::Write)?;
                on_progress(status_from_checkpoint(&self.checkpoint, false));
                return Ok(None);
            }

            let read = source.read(&mut buffer).map_err(|err| match self.source {
                DownloadSource::Http(_) => net::classify_body_error(err),
                DownloadSource::File(_) => ModelError::Read(err),
            })?;
            if read == 0 {
                return Err(ModelError::CorruptSource {
                    model_id: self.manifest.model_id.clone(),
                    expected: self.checkpoint.total_bytes,
                });
            }
            // Never write past the expected size; a wrong file fails the hash.
            let remaining = self.checkpoint.total_bytes - self.checkpoint.downloaded_bytes;
            let read = read.min(usize::try_from(remaining).unwrap_or(usize::MAX));

            let next_total = self.checkpoint.downloaded_bytes + read as u64;
            if let Some(limit) = self.max_storage_bytes {
                if next_total > limit {
                    return Err(ModelError::LowDisk {
                        model_id: self.manifest.model_id.clone(),
                        limit,
                    });
                }
            }

            if let Err(write_err) = partial_file.write_all(&buffer[..read]) {
                if net::is_disk_full(&write_err) {
                    return Err(ModelError::LowDisk {
                        model_id: self.manifest.model_id.clone(),
                        limit: next_total,
                    });
                }
                return Err(ModelError::Write(write_err));
            }
            hasher.update(&buffer[..read]);

            self.checkpoint.downloaded_bytes = next_total;
            self.checkpoint.state = ModelStatusState::Downloading;
            self.checkpoint.last_error = None;
            self.checkpoint.failure_stage = None;
            self.checkpoint.updated_at_utc_ms = now_utc_ms();
            if last_emit.is_none_or(|at| at.elapsed() >= PROGRESS_EMIT_INTERVAL) {
                on_progress(status_from_checkpoint(&self.checkpoint, false));
                last_emit = Some(Instant::now());
            }
        }

        // Flushed to disk before it can be promoted: a rename alone can
        // publish a zero-filled file after a power loss.
        partial_file.sync_all().map_err(ModelError::Write)?;
        Ok(Some(to_hex(&hasher.finalize())))
    }

    /// Records a cancel or pause request in the checkpoint.
    fn stop_requested<C, P>(&mut self, should_cancel: &mut C, should_pause: &mut P) -> bool
    where
        C: FnMut() -> bool,
        P: FnMut() -> bool,
    {
        let (state, message) = if should_cancel() {
            (ModelStatusState::Cancelled, "Download cancelled by user.")
        } else if should_pause() {
            (ModelStatusState::Paused, "Download paused.")
        } else {
            return false;
        };
        self.checkpoint.state = state;
        self.checkpoint.last_error = Some(message.to_string());
        self.checkpoint.updated_at_utc_ms = now_utc_ms();
        true
    }

    /// Opens the source at the resume offset. A server that ignores the
    /// Range request gets a clean restart: partial truncated, hash reset.
    fn open_source(
        &mut self,
        partial_file: &mut fs::File,
        hasher: &mut Sha256,
    ) -> Result<Box<dyn Read>, ModelError> {
        let offset = self.checkpoint.downloaded_bytes;
        match &self.source {
            DownloadSource::File(path) => {
                let mut file = fs::File::open(path).map_err(|err| {
                    if err.kind() == io::ErrorKind::NotFound {
                        ModelError::SourceMissing {
                            model_id: self.manifest.model_id.clone(),
                            expected_path: path.to_string_lossy().to_string(),
                        }
                    } else {
                        ModelError::Read(err)
                    }
                })?;
                file.seek(SeekFrom::Start(offset))
                    .map_err(ModelError::Read)?;
                Ok(Box::new(file))
            }
            DownloadSource::Http(url) => {
                let mut request = net::http_agent().get(url);
                if offset > 0 {
                    request = request.set("Range", &format!("bytes={offset}-"));
                }
                let response = request.call().map_err(net::classify_request_error)?;
                if offset > 0 && response.status() != 206 {
                    // A 200 carries the whole file from byte 0.
                    partial_file.set_len(0).map_err(ModelError::Write)?;
                    partial_file
                        .seek(SeekFrom::Start(0))
                        .map_err(ModelError::Write)?;
                    *hasher = Sha256::new();
                    self.checkpoint.downloaded_bytes = 0;
                    self.checkpoint.last_error =
                        Some("Source does not support resume; restarted download.".to_string());
                }
                Ok(Box::new(response.into_reader()))
            }
        }
    }
}

pub struct ModelManager {
    model_dir: PathBuf,
    source_dir: PathBuf,
    quarantine_dir: PathBuf,
    installed_index_path: PathBuf,
    download_state_path: PathBuf,
    catalog: Vec<SignedModelManifest>,
    installed: HashMap<String, InstalledModel>,
    downloads: HashMap<String, DownloadCheckpoint>,
    max_storage_bytes: Option<u64>,
    download_chunk_bytes: usize,
}

impl ModelManager {
    fn default_download_chunk_bytes() -> usize {
        let fallback = 8 * 1024 * 1024;
        if let Ok(value) = env::var("VOICEWAVE_DOWNLOAD_CHUNK_MB") {
            if let Ok(parsed) = value.trim().parse::<usize>() {
                if parsed > 0 {
                    return parsed.saturating_mul(1024 * 1024);
                }
            }
        }
        fallback
    }

    pub fn new() -> Result<Self, ModelError> {
        let proj_dirs =
            ProjectDirs::from("com", "voicewave", "localcore").ok_or(ModelError::AppData)?;
        let model_dir = proj_dirs.data_dir().join("models");
        let source_dir = proj_dirs.data_dir().join("model-sources");
        let quarantine_dir = proj_dirs.data_dir().join("model-quarantine");
        let installed_index_path = proj_dirs.config_dir().join("models.json");
        let download_state_path = proj_dirs.config_dir().join("model-downloads.json");

        let mut manager = Self {
            model_dir,
            source_dir,
            quarantine_dir,
            installed_index_path,
            download_state_path,
            catalog: build_catalog(),
            installed: HashMap::new(),
            downloads: HashMap::new(),
            max_storage_bytes: None,
            download_chunk_bytes: Self::default_download_chunk_bytes(),
        };

        manager.load_installed()?;
        manager.reconcile_installed()?;
        manager.load_downloads()?;
        manager.ensure_local_sources()?;
        manager.apply_source_overrides()?;
        manager.reconcile_downloads_after_restart()?;
        Ok(manager)
    }

    /// What this PC is offered (plan 019): always two rows, "Small" and
    /// "Large Turbo", from the machine's model family, plus any installed
    /// model of the other family (an upgrader's faster-whisper fallback, or
    /// a GGML model after the GPU went away) so it stays usable and
    /// removable. `active_model` picks the copy for the row in use. Parakeet,
    /// the recommended row, is on every PC that can run the speech workers;
    /// `parakeet_on` is where it runs (see `model_fit`).
    pub fn list_catalog(
        &self,
        hardware: &HardwareProfile,
        active_model: &str,
        parakeet_on: Option<AsrRung>,
    ) -> Vec<ModelCatalogItem> {
        let primary = [
            hardware.model_id(ModelTier::Small),
            hardware.model_id(ModelTier::LargeTurbo),
        ];
        let workers_run = hardware.cpu == CpuSupport::Full;
        self.catalog
            .iter()
            .filter(|manifest| {
                primary.contains(&manifest.model_id.as_str())
                    || self.installed.contains_key(&manifest.model_id)
                    || (workers_run && manifest.model_id == PARAKEET_MODEL_ID)
            })
            .filter_map(|manifest| {
                let active = manifest.model_id == active_model;
                manifest_to_catalog_item(manifest, Some(hardware), active, parakeet_on)
            })
            .collect()
    }

    /// Every catalog model id, both families.
    pub fn catalog_model_ids(&self) -> Vec<String> {
        self.catalog
            .iter()
            .map(|manifest| manifest.model_id.clone())
            .collect()
    }

    pub fn list_installed(&self) -> Vec<InstalledModel> {
        let mut rows: Vec<_> = self.installed.values().cloned().collect();
        rows.sort_by_key(|row| row.installed_at_utc_ms);
        rows
    }

    pub fn remove_installed(&mut self, model_id: &str) -> Result<(), ModelError> {
        if self.installed.remove(model_id).is_some() {
            self.persist_installed()?;
        }
        Ok(())
    }

    pub fn install_faster_whisper_model(
        &mut self,
        model_id: &str,
        cache_path: &str,
    ) -> Result<InstalledModel, ModelError> {
        let manifest = self
            .catalog
            .iter()
            .find(|item| item.model_id == model_id)
            .ok_or_else(|| ModelError::UnknownModel(model_id.to_string()))?;
        if !is_virtual_model_format(&manifest.format) {
            return Err(ModelError::UnsupportedModelFormat {
                model_id: model_id.to_string(),
                format: manifest.format.clone(),
            });
        }

        let cache_path = PathBuf::from(cache_path);
        if !cache_path.exists() {
            return Err(ModelError::SourceMissing {
                model_id: model_id.to_string(),
                expected_path: cache_path.to_string_lossy().to_string(),
            });
        }
        // Callers get here after the worker loaded the model (proving
        // model.bin/config parse) or when adopting an existing cache at
        // startup. This catches the files a successful load does NOT prove
        // are on disk (e.g. a missing tokenizer.json makes
        // faster-whisper silently fetch one online, which then fails offline).
        verify_faster_whisper_snapshot(&cache_path).map_err(|reason| {
            ModelError::IncompleteModel {
                model_id: model_id.to_string(),
                reason,
            }
        })?;

        let size_bytes = directory_size_bytes(&cache_path).unwrap_or(manifest.size);
        let installed = InstalledModel {
            model_id: manifest.model_id.clone(),
            version: manifest.version.clone(),
            format: manifest.format.clone(),
            size_bytes,
            file_path: cache_path.to_string_lossy().to_string(),
            // HuggingFace serves these models with no hash we pin, so there
            // is nothing honest to record or verify against.
            sha256: String::new(),
            installed_at_utc_ms: now_utc_ms(),
            checksum_verified: false,
        };
        self.installed
            .insert(installed.model_id.clone(), installed.clone());
        self.persist_installed()?;
        Ok(installed)
    }

    /// Any catalog model, either family, without machine-specific copy.
    pub fn get_catalog_item(&self, model_id: &str) -> Option<ModelCatalogItem> {
        self.catalog
            .iter()
            .find(|item| item.model_id == model_id)
            .and_then(|manifest| manifest_to_catalog_item(manifest, None, false, None))
    }

    /// Delete an installed model's files and unregister it. Only paths inside
    /// VoiceWave's own data directory are ever deleted.
    pub fn remove_model_files(&mut self, model_id: &str) -> Result<(), ModelError> {
        let Some(installed) = self.installed.get(model_id).cloned() else {
            return Ok(());
        };
        let path = PathBuf::from(&installed.file_path);
        // A faster-whisper model lives in a Hugging Face cache repo directory
        // ("models--org--name"); older installs registered that directory,
        // newer ones its snapshot. Remove the whole repo either way.
        let target = if is_virtual_model_format(&installed.format) {
            path.ancestors()
                .find(|dir| {
                    dir.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.starts_with("models--"))
                })
                .map(Path::to_path_buf)
        } else {
            Some(path)
        };
        let data_root = self
            .model_dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.model_dir.clone());
        if let Some(target) = target.filter(|target| target.starts_with(&data_root)) {
            let removed = if target.is_dir() {
                fs::remove_dir_all(&target)
            } else {
                fs::remove_file(&target)
            };
            match removed {
                Ok(()) => {}
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(err) => return Err(ModelError::Write(err)),
            }
        }
        self.remove_installed(model_id)
    }

    pub fn get_installed(&self, model_id: &str) -> Option<InstalledModel> {
        self.installed.get(model_id).cloned()
    }

    pub fn get_download_status(
        &self,
        model_id: &str,
        active_model: Option<&str>,
    ) -> Option<ModelStatus> {
        if let Some(installed) = self.get_installed(model_id) {
            return Some(ModelStatus {
                model_id: model_id.to_string(),
                state: ModelStatusState::Installed,
                progress: 100,
                active: active_model.is_some_and(|value| value == model_id),
                installed: true,
                message: Some(installed_status_message(&installed).to_string()),
                installed_model: Some(installed),
                downloaded_bytes: None,
                total_bytes: None,
                resumable: false,
                failure_stage: None,
            });
        }

        if let Some(checkpoint) = self.downloads.get(model_id) {
            return Some(status_from_checkpoint(
                checkpoint,
                active_model.is_some_and(|value| value == model_id),
            ));
        }

        self.get_catalog_item(model_id).map(|_| ModelStatus {
            model_id: model_id.to_string(),
            state: ModelStatusState::Idle,
            progress: 0,
            active: active_model.is_some_and(|value| value == model_id),
            installed: false,
            message: Some("Not installed.".to_string()),
            installed_model: None,
            downloaded_bytes: Some(0),
            total_bytes: self
                .catalog
                .iter()
                .find(|item| item.model_id == model_id)
                .map(|item| item.size),
            resumable: false,
            failure_stage: None,
        })
    }

    /// Download, verify and install a model in one call while holding `&mut
    /// self` throughout. The app splits these phases so the `model_manager`
    /// lock is not held during the download (see `state.rs` download_model).
    pub fn install_model_resumable<F, C, P>(
        &mut self,
        model_id: &str,
        should_cancel: C,
        should_pause: P,
        mut on_progress: F,
    ) -> Result<ModelStatus, ModelError>
    where
        F: FnMut(ModelStatus),
        C: FnMut() -> bool,
        P: FnMut() -> bool,
    {
        let mut plan = self.begin_download(model_id)?;
        let outcome = plan.run(should_cancel, should_pause, &mut on_progress);
        let status = self.finish_download(plan, outcome)?;
        if status.installed {
            on_progress(status.clone());
        }
        Ok(status)
    }

    /// Phase 1 (needs the lock): validate the signed manifest, resolve the
    /// source and restore the resume checkpoint from the partial file.
    pub fn begin_download(&mut self, model_id: &str) -> Result<DownloadPlan, ModelError> {
        let manifest = self
            .catalog
            .iter()
            .find(|item| item.model_id == model_id)
            .cloned()
            .ok_or_else(|| ModelError::UnknownModel(model_id.to_string()))?;

        if !is_supported_format(&manifest.format) {
            return Err(ModelError::UnsupportedModelFormat {
                model_id: model_id.to_string(),
                format: manifest.format,
            });
        }

        // The signature covers the pinned sha256, size and URL, so a tampered
        // catalog entry cannot redirect the download or relax the hash check.
        if validate_manifest_signature(&manifest).is_err() {
            if let Some(checkpoint) = self.downloads.remove(model_id) {
                let _ = quarantine_file(
                    &self.quarantine_dir,
                    Path::new(&checkpoint.partial_path),
                    model_id,
                    "manifest-signature",
                );
                let _ = fs::remove_file(&checkpoint.partial_path);
                let _ = self.persist_downloads();
            }
            // Also quarantine any orphan partial artifact if checkpoint metadata is missing.
            let orphan_partial = self.partial_model_path_for(&manifest);
            if orphan_partial.exists() {
                let _ = quarantine_file(
                    &self.quarantine_dir,
                    &orphan_partial,
                    model_id,
                    "manifest-signature",
                );
                let _ = fs::remove_file(&orphan_partial);
            }
            return Err(ModelError::ManifestSignatureInvalid {
                model_id: model_id.to_string(),
            });
        }

        fs::create_dir_all(self.model_dir.join("downloads")).map_err(ModelError::Write)?;
        fs::create_dir_all(&self.quarantine_dir).map_err(ModelError::Write)?;

        let source = if is_http_url(&manifest.download_url) {
            DownloadSource::Http(manifest.download_url.clone())
        } else {
            let source_path = self.resolve_download_source(&manifest)?;
            let source_len = fs::metadata(&source_path)
                .map_err(|err| {
                    if err.kind() == io::ErrorKind::NotFound {
                        ModelError::SourceMissing {
                            model_id: model_id.to_string(),
                            expected_path: source_path.to_string_lossy().to_string(),
                        }
                    } else {
                        ModelError::Read(err)
                    }
                })?
                .len();
            if source_len != manifest.size {
                return Err(ModelError::ManifestSizeMismatch {
                    model_id: model_id.to_string(),
                    expected: manifest.size,
                    actual: source_len,
                });
            }
            DownloadSource::File(source_path)
        };

        let checkpoint = self.prepare_checkpoint(&manifest)?;
        Ok(DownloadPlan {
            manifest,
            checkpoint,
            source,
            quarantine_dir: self.quarantine_dir.clone(),
            max_storage_bytes: self.max_storage_bytes,
            chunk_bytes: self.download_chunk_bytes.max(4 * 1024),
        })
    }

    /// Phase 3 (needs the lock): record how `DownloadPlan::run` ended. Only a
    /// verified artifact is moved into place, and `fs::rename` replaces the
    /// old file in one step (on Windows it fails without touching the target
    /// if that file is in use), so a working model is never deleted before
    /// its replacement is ready.
    pub fn finish_download(
        &mut self,
        mut plan: DownloadPlan,
        outcome: Result<DownloadOutcome, ModelError>,
    ) -> Result<ModelStatus, ModelError> {
        let model_id = plan.manifest.model_id.clone();
        let verified_sha256 = match outcome {
            Ok(DownloadOutcome::Verified { sha256 }) => sha256,
            Ok(DownloadOutcome::Stopped) => {
                self.downloads.insert(model_id, plan.checkpoint.clone());
                self.persist_downloads()?;
                return Ok(status_from_checkpoint(&plan.checkpoint, false));
            }
            Err(err) => {
                // `run` already logged the raw error; the status gets the
                // plain message for its stage.
                plan.checkpoint.state = ModelStatusState::Failed;
                plan.checkpoint.last_error = Some(err.user_message());
                plan.checkpoint.failure_stage = Some(err.failure_stage());
                plan.checkpoint.updated_at_utc_ms = now_utc_ms();
                self.downloads.insert(model_id, plan.checkpoint);
                self.persist_downloads()?;
                return Err(err);
            }
        };

        let final_path = self.final_model_path_for(&plan.manifest);
        if let Err(err) = fs::rename(&plan.checkpoint.partial_path, &final_path) {
            // The verified partial stays put; a retry re-hashes it and tries
            // the move again without downloading anything.
            let err = ModelError::Rename(err);
            tracing::warn!("{model_id}: {err}");
            plan.checkpoint.state = ModelStatusState::Failed;
            plan.checkpoint.last_error = Some(err.user_message());
            plan.checkpoint.failure_stage = Some(err.failure_stage());
            plan.checkpoint.updated_at_utc_ms = now_utc_ms();
            self.downloads.insert(model_id, plan.checkpoint);
            self.persist_downloads()?;
            return Err(err);
        }

        let installed = InstalledModel {
            model_id: plan.manifest.model_id.clone(),
            version: plan.manifest.version.clone(),
            format: plan.manifest.format.clone(),
            size_bytes: plan.manifest.size,
            file_path: final_path.to_string_lossy().to_string(),
            sha256: verified_sha256,
            installed_at_utc_ms: now_utc_ms(),
            checksum_verified: true,
        };

        self.installed
            .insert(installed.model_id.clone(), installed.clone());
        self.persist_installed()?;
        self.downloads.remove(&model_id);
        self.persist_downloads()?;

        Ok(ModelStatus {
            model_id,
            state: ModelStatusState::Installed,
            progress: 100,
            active: false,
            installed: true,
            message: Some(
                "Model installed successfully with manifest + checksum verification.".to_string(),
            ),
            installed_model: Some(installed),
            downloaded_bytes: Some(plan.manifest.size),
            total_bytes: Some(plan.manifest.size),
            resumable: false,
            failure_stage: None,
        })
    }

    #[cfg(test)]
    fn with_test_paths(root: &Path) -> Result<Self, ModelError> {
        let model_dir = root.join("models");
        let source_dir = root.join("source-models");
        let quarantine_dir = root.join("quarantine");
        let installed_index_path = root.join("models.json");
        let download_state_path = root.join("model-downloads.json");

        fs::create_dir_all(&model_dir).map_err(ModelError::Write)?;
        fs::create_dir_all(&source_dir).map_err(ModelError::Write)?;
        fs::create_dir_all(&quarantine_dir).map_err(ModelError::Write)?;

        let mut manager = Self {
            model_dir,
            source_dir,
            quarantine_dir,
            installed_index_path,
            download_state_path,
            catalog: build_catalog(),
            installed: HashMap::new(),
            downloads: HashMap::new(),
            max_storage_bytes: None,
            download_chunk_bytes: 8 * 1024,
        };
        manager.ensure_local_sources()?;
        manager.apply_source_overrides()?;
        Ok(manager)
    }

    #[cfg(test)]
    fn set_test_storage_limit(&mut self, max_storage_bytes: Option<u64>) {
        self.max_storage_bytes = max_storage_bytes;
    }

    #[cfg(test)]
    fn set_test_chunk_size(&mut self, chunk_size: usize) {
        self.download_chunk_bytes = chunk_size.max(1024);
    }

    fn prepare_checkpoint(
        &mut self,
        manifest: &SignedModelManifest,
    ) -> Result<DownloadCheckpoint, ModelError> {
        let partial_path = self.partial_model_path_for(manifest);
        let partial_path_str = partial_path.to_string_lossy().to_string();

        let mut checkpoint =
            self.downloads
                .get(&manifest.model_id)
                .cloned()
                .unwrap_or(DownloadCheckpoint {
                    model_id: manifest.model_id.clone(),
                    version: manifest.version.clone(),
                    partial_path: partial_path_str,
                    downloaded_bytes: 0,
                    total_bytes: manifest.size,
                    state: ModelStatusState::Idle,
                    last_error: None,
                    failure_stage: None,
                    updated_at_utc_ms: now_utc_ms(),
                });

        if checkpoint.version != manifest.version {
            let old_path = PathBuf::from(&checkpoint.partial_path);
            let _ = quarantine_file(
                &self.quarantine_dir,
                &old_path,
                &manifest.model_id,
                "version-drift",
            );
            let _ = fs::remove_file(old_path);
            checkpoint.version = manifest.version.clone();
            checkpoint.partial_path = partial_path.to_string_lossy().to_string();
            checkpoint.downloaded_bytes = 0;
            checkpoint.state = ModelStatusState::Idle;
            checkpoint.last_error = None;
        }

        // The partial file's length is the resume point: it is what actually
        // reached disk, whatever the last persisted checkpoint said.
        checkpoint.last_error = None;
        checkpoint.failure_stage = None;
        if partial_path.exists() {
            let file_size = fs::metadata(&partial_path).map_err(ModelError::Read)?.len();
            if file_size > manifest.size {
                let _ = quarantine_file(
                    &self.quarantine_dir,
                    &partial_path,
                    &manifest.model_id,
                    "oversized-partial",
                );
                let _ = fs::remove_file(&partial_path);
                checkpoint.downloaded_bytes = 0;
                checkpoint.last_error =
                    Some("Corrupt partial artifact detected and quarantined.".to_string());
            } else {
                checkpoint.downloaded_bytes = file_size;
            }
        } else {
            checkpoint.downloaded_bytes = 0;
        }

        checkpoint.total_bytes = manifest.size;
        checkpoint.state = ModelStatusState::Downloading;
        checkpoint.updated_at_utc_ms = now_utc_ms();

        self.downloads
            .insert(manifest.model_id.clone(), checkpoint.clone());
        self.persist_downloads()?;
        Ok(checkpoint)
    }

    fn load_installed(&mut self) -> Result<(), ModelError> {
        // `read_to_string_recovering` finishes an interrupted replace and
        // reports a zero-filled file as absent — a plain `trim()` does not,
        // because Rust never classifies `\0` as whitespace.
        let raw = match atomic_file::read_to_string_recovering(&self.installed_index_path) {
            Ok(Some(raw)) => raw,
            Ok(None) => {
                if !self.installed_index_path.exists() {
                    return Ok(());
                }
                self.installed.clear();
                self.persist_installed()?;
                return Ok(());
            }
            Err(error) => {
                tracing::warn!(
                    "failed to read the installed model index at '{}': {}",
                    self.installed_index_path.display(),
                    error
                );
                return Ok(());
            }
        };
        let normalized = raw.trim_start_matches('\u{feff}').trim();
        let decoded = serde_json::from_str::<InstalledModelStore>(normalized).or_else(|_| {
            serde_json::from_str::<Vec<InstalledModel>>(normalized)
                .map(|installed| InstalledModelStore { installed })
        });
        match decoded {
            Ok(decoded) => {
                self.installed = decoded
                    .installed
                    .into_iter()
                    .map(|row| (row.model_id.clone(), row))
                    .collect();
            }
            Err(err) => {
                tracing::warn!(
                    "corrupt installed model index recovered at '{}': {}",
                    self.installed_index_path.display(),
                    err
                );
                let _ = quarantine_metadata_file(&self.installed_index_path, "installed-corrupt");
                self.installed.clear();
                self.persist_installed()?;
            }
        }
        Ok(())
    }

    fn reconcile_installed(&mut self) -> Result<(), ModelError> {
        let mut changed = false;
        let mut to_remove = Vec::new();
        let supported_ids = self
            .catalog
            .iter()
            .map(|manifest| manifest.model_id.as_str())
            .collect::<std::collections::HashSet<_>>();
        for (model_id, model) in &self.installed {
            if !supported_ids.contains(model_id.as_str()) {
                to_remove.push(model_id.clone());
                changed = true;
                continue;
            }
            if is_virtual_model_format(&model.format) {
                // Files deleted or never finished: un-register so the UI
                // offers a (resuming) download instead of a dead model.
                if let Err(reason) = verify_faster_whisper_snapshot(Path::new(&model.file_path)) {
                    tracing::warn!("{model_id} no longer installed: {reason}");
                    to_remove.push(model_id.clone());
                    changed = true;
                }
                continue;
            }
            let path = Path::new(&model.file_path);
            if !path.exists() {
                to_remove.push(model_id.clone());
                changed = true;
                continue;
            }
            match fs::metadata(path) {
                Ok(meta) => {
                    if meta.len() != model.size_bytes {
                        let _ =
                            quarantine_file(&self.quarantine_dir, path, model_id, "size-mismatch");
                        let _ = fs::remove_file(path);
                        to_remove.push(model_id.clone());
                        changed = true;
                    }
                }
                Err(_) => {
                    to_remove.push(model_id.clone());
                    changed = true;
                }
            }
        }

        if !to_remove.is_empty() {
            for model_id in to_remove {
                self.installed.remove(&model_id);
            }
        }

        if changed {
            self.persist_installed()?;
        }
        Ok(())
    }

    fn persist_installed(&self) -> Result<(), ModelError> {
        let payload = InstalledModelStore {
            installed: self.list_installed(),
        };
        let encoded = serde_json::to_string_pretty(&payload).map_err(ModelError::Parse)?;
        atomic_file::atomic_write(&self.installed_index_path, encoded.as_bytes())
            .map_err(ModelError::Write)?;
        Ok(())
    }

    fn load_downloads(&mut self) -> Result<(), ModelError> {
        let raw = match atomic_file::read_to_string_recovering(&self.download_state_path) {
            Ok(Some(raw)) => raw,
            Ok(None) => {
                if !self.download_state_path.exists() {
                    return Ok(());
                }
                self.downloads.clear();
                self.persist_downloads()?;
                return Ok(());
            }
            Err(error) => {
                tracing::warn!(
                    "failed to read the model download checkpoints at '{}': {}",
                    self.download_state_path.display(),
                    error
                );
                return Ok(());
            }
        };
        let normalized = raw.trim_start_matches('\u{feff}').trim();
        let decoded = serde_json::from_str::<DownloadCheckpointStore>(normalized).or_else(|_| {
            serde_json::from_str::<Vec<DownloadCheckpoint>>(normalized)
                .map(|downloads| DownloadCheckpointStore { downloads })
        });
        match decoded {
            Ok(decoded) => {
                self.downloads = decoded
                    .downloads
                    .into_iter()
                    .map(|row| (row.model_id.clone(), row))
                    .collect();
            }
            Err(err) => {
                tracing::warn!(
                    "corrupt model download checkpoint recovered at '{}': {}",
                    self.download_state_path.display(),
                    err
                );
                let _ = quarantine_metadata_file(&self.download_state_path, "downloads-corrupt");
                self.downloads.clear();
                self.persist_downloads()?;
            }
        }
        Ok(())
    }

    fn persist_downloads(&self) -> Result<(), ModelError> {
        let payload = DownloadCheckpointStore {
            downloads: self.downloads.values().cloned().collect(),
        };
        let encoded = serde_json::to_string_pretty(&payload).map_err(ModelError::Parse)?;
        atomic_file::atomic_write(&self.download_state_path, encoded.as_bytes())
            .map_err(ModelError::Write)?;
        Ok(())
    }

    fn ensure_local_sources(&self) -> Result<(), ModelError> {
        if !allow_synthetic_sources() {
            return Ok(());
        }

        fs::create_dir_all(&self.source_dir).map_err(ModelError::Write)?;
        for manifest in &self.catalog {
            if !manifest
                .download_url
                .starts_with("local://model-artifacts/")
            {
                continue;
            }
            let path = self.source_path_for_manifest(manifest);
            let payload =
                source_payload_for_manifest(&manifest.model_id, &manifest.version, manifest.size);
            let needs_write = if path.exists() {
                let existing = fs::read(&path).map_err(ModelError::Read)?;
                sha256_hex(&existing) != manifest.sha256
            } else {
                true
            };

            if needs_write {
                fs::write(&path, &payload).map_err(ModelError::Write)?;
            }
        }
        Ok(())
    }

    fn apply_source_overrides(&mut self) -> Result<(), ModelError> {
        let env_source_dir = env::var("VOICEWAVE_MODEL_SOURCE_DIR")
            .ok()
            .map(PathBuf::from);
        let local_source_dir = self.source_dir.clone();
        for manifest in &mut self.catalog {
            if let Some(source_path) = resolve_override_source_path_for_manifest(
                manifest,
                env_source_dir.as_ref(),
                &local_source_dir,
            ) {
                let format = source_path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| {
                        if ext.eq_ignore_ascii_case("gguf") {
                            "gguf"
                        } else {
                            "bin"
                        }
                    })
                    .unwrap_or("bin");
                manifest.size = fs::metadata(&source_path).map_err(ModelError::Read)?.len();
                manifest.sha256 = sha256_file(&source_path).map_err(ModelError::Read)?;
                manifest.format = format.to_string();
                manifest.download_url = format!("file://{}", source_path.to_string_lossy());
                manifest.signature = sign_manifest(manifest);
            }
        }
        Ok(())
    }

    fn resolve_download_source(
        &self,
        manifest: &SignedModelManifest,
    ) -> Result<PathBuf, ModelError> {
        if manifest
            .download_url
            .starts_with("local://model-artifacts/")
        {
            return Ok(self
                .resolve_override_source_path(manifest)
                .unwrap_or_else(|| self.source_path_for_manifest(manifest)));
        }

        if let Some(path) = manifest.download_url.strip_prefix("file://") {
            return Ok(PathBuf::from(path));
        }

        Err(ModelError::UnsupportedDownloadUrl {
            model_id: manifest.model_id.clone(),
            url: manifest.download_url.clone(),
        })
    }

    fn source_path_for_manifest(&self, manifest: &SignedModelManifest) -> PathBuf {
        let ext = model_extension_for(&manifest.format);
        self.source_dir
            .join(format!("{}.{}", manifest.model_id, ext))
    }

    fn partial_model_path_for(&self, manifest: &SignedModelManifest) -> PathBuf {
        self.model_dir.join("downloads").join(format!(
            "{}-{}.part",
            manifest.model_id.replace('.', "_"),
            manifest.version.replace('.', "_")
        ))
    }

    fn final_model_path_for(&self, manifest: &SignedModelManifest) -> PathBuf {
        let ext = model_extension_for(&manifest.format);
        self.model_dir.join(format!(
            "{}-{}.{}",
            manifest.model_id.replace('.', "_"),
            manifest.version.replace('.', "_"),
            ext
        ))
    }

    fn reconcile_downloads_after_restart(&mut self) -> Result<(), ModelError> {
        let mut changed = false;
        let mut quarantine_jobs: Vec<(PathBuf, String)> = Vec::new();
        for checkpoint in self.downloads.values_mut() {
            let manifest = self
                .catalog
                .iter()
                .find(|row| row.model_id == checkpoint.model_id);
            if let Some(manifest) = manifest {
                if checkpoint.total_bytes != manifest.size {
                    checkpoint.total_bytes = manifest.size;
                    changed = true;
                }
            }

            let partial_path = Path::new(&checkpoint.partial_path);
            if partial_path.exists() {
                match fs::metadata(partial_path) {
                    Ok(meta) => {
                        let size = meta.len();
                        if size > checkpoint.total_bytes && checkpoint.total_bytes > 0 {
                            quarantine_jobs
                                .push((partial_path.to_path_buf(), checkpoint.model_id.clone()));
                            checkpoint.downloaded_bytes = 0;
                            checkpoint.state = ModelStatusState::Failed;
                            checkpoint.last_error = Some(
                                "Corrupt partial artifact detected and quarantined.".to_string(),
                            );
                            checkpoint.updated_at_utc_ms = now_utc_ms();
                            changed = true;
                        } else if size != checkpoint.downloaded_bytes {
                            checkpoint.downloaded_bytes = size;
                            checkpoint.updated_at_utc_ms = now_utc_ms();
                            changed = true;
                        }
                    }
                    Err(_) => {
                        checkpoint.downloaded_bytes = 0;
                        checkpoint.state = ModelStatusState::Failed;
                        checkpoint.last_error =
                            Some("Failed to read partial download metadata.".to_string());
                        checkpoint.updated_at_utc_ms = now_utc_ms();
                        changed = true;
                    }
                }
            } else if checkpoint.downloaded_bytes > 0 {
                checkpoint.downloaded_bytes = 0;
                checkpoint.state = ModelStatusState::Idle;
                checkpoint.last_error =
                    Some("Download checkpoint reset (partial file missing).".to_string());
                checkpoint.updated_at_utc_ms = now_utc_ms();
                changed = true;
            }

            if matches!(checkpoint.state, ModelStatusState::Downloading) {
                checkpoint.state = ModelStatusState::Paused;
                checkpoint.last_error =
                    Some("Resumable checkpoint restored after restart.".to_string());
                checkpoint.updated_at_utc_ms = now_utc_ms();
                changed = true;
            }
        }

        for (partial_path, model_id) in quarantine_jobs {
            let _ = quarantine_file(
                &self.quarantine_dir,
                &partial_path,
                &model_id,
                "oversized-partial",
            );
            let _ = fs::remove_file(&partial_path);
        }
        if changed {
            self.persist_downloads()?;
        }
        Ok(())
    }

    fn resolve_override_source_path(&self, manifest: &SignedModelManifest) -> Option<PathBuf> {
        let env_source_dir = env::var("VOICEWAVE_MODEL_SOURCE_DIR")
            .ok()
            .map(PathBuf::from);
        resolve_override_source_path_for_manifest(
            manifest,
            env_source_dir.as_ref(),
            &self.source_dir,
        )
    }
}

fn resolve_override_source_path_for_manifest(
    manifest: &SignedModelManifest,
    env_source_dir: Option<&PathBuf>,
    local_source_dir: &Path,
) -> Option<PathBuf> {
    let preferred_ext = model_extension_for(&manifest.format);
    let fallback_ext = if preferred_ext == "bin" {
        "gguf"
    } else {
        "bin"
    };
    let extensions = [preferred_ext, fallback_ext];

    for ext in extensions {
        let file_name = format!("{}.{}", manifest.model_id, ext);
        if let Some(source_dir) = env_source_dir {
            let candidate = source_dir.join(&file_name);
            if candidate.exists() {
                return Some(candidate);
            }
        }
        let candidate = local_source_dir.join(&file_name);
        if candidate.exists() {
            return Some(candidate);
        }
    }

    None
}

fn status_from_checkpoint(checkpoint: &DownloadCheckpoint, active: bool) -> ModelStatus {
    let progress = if checkpoint.total_bytes == 0 {
        0
    } else {
        ((checkpoint.downloaded_bytes.saturating_mul(100)) / checkpoint.total_bytes).min(100) as u8
    };

    let resumable = matches!(
        checkpoint.state,
        ModelStatusState::Downloading
            | ModelStatusState::Paused
            | ModelStatusState::Cancelled
            | ModelStatusState::Failed
    );

    ModelStatus {
        model_id: checkpoint.model_id.clone(),
        state: checkpoint.state.clone(),
        progress,
        active,
        installed: false,
        message: checkpoint
            .last_error
            .clone()
            .or_else(|| Some("Model download checkpoint restored.".to_string())),
        installed_model: None,
        downloaded_bytes: Some(checkpoint.downloaded_bytes),
        total_bytes: Some(checkpoint.total_bytes),
        resumable,
        failure_stage: checkpoint
            .failure_stage
            .filter(|_| checkpoint.state == ModelStatusState::Failed),
    }
}

/// Moves `path` into `quarantine_dir` (kept for inspection, out of the way of
/// the next attempt). Returns the new location, or None if `path` is absent.
fn quarantine_file(
    quarantine_dir: &Path,
    path: &Path,
    model_id: &str,
    reason: &str,
) -> Result<Option<PathBuf>, ModelError> {
    if !path.exists() {
        return Ok(None);
    }
    fs::create_dir_all(quarantine_dir).map_err(ModelError::Write)?;

    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("bin");
    let target = quarantine_dir.join(format!(
        "{}-{}-{}.{}",
        model_id.replace('.', "_"),
        reason,
        now_utc_ms(),
        extension
    ));

    match fs::rename(path, &target) {
        Ok(_) => Ok(Some(target)),
        Err(_) => {
            fs::copy(path, &target).map_err(ModelError::Write)?;
            fs::remove_file(path).map_err(ModelError::Write)?;
            Ok(Some(target))
        }
    }
}

fn now_utc_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

fn quarantine_metadata_file(path: &Path, reason: &str) -> io::Result<PathBuf> {
    if !path.exists() {
        return Ok(path.to_path_buf());
    }
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("metadata");
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("json");
    let backup_name = format!("{stem}.{reason}.{}.{}", now_utc_ms(), ext);
    let backup_path = path.with_file_name(backup_name);
    fs::rename(path, &backup_path)?;
    Ok(backup_path)
}

fn directory_size_bytes(path: &Path) -> Option<u64> {
    let mut total = 0_u64;
    let entries = fs::read_dir(path).ok()?;
    for entry in entries.flatten() {
        let entry_path = entry.path();
        // symlink_metadata: HF snapshots can be symlinks into blobs/, and
        // following them would count every model file twice.
        let metadata = fs::symlink_metadata(&entry_path).ok()?;
        if metadata.is_file() {
            total = total.saturating_add(metadata.len());
        } else if metadata.is_dir() {
            total = total.saturating_add(directory_size_bytes(&entry_path)?);
        }
    }
    Some(total)
}

fn allow_synthetic_sources() -> bool {
    cfg!(test)
        || env::var("VOICEWAVE_SYNTHETIC_MODEL_SOURCES")
            .map(|value| value == "1")
            .unwrap_or(false)
}

fn is_supported_format(format: &str) -> bool {
    let normalized = format.trim().to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "gguf" | "bin" | "ggml" | "ggml-bin" | "faster-whisper"
    )
}

fn is_virtual_model_format(format: &str) -> bool {
    format.trim().eq_ignore_ascii_case(FASTER_WHISPER_FORMAT)
}

fn model_extension_for(format: &str) -> &'static str {
    if format.trim().eq_ignore_ascii_case("gguf") {
        "gguf"
    } else {
        "bin"
    }
}

fn is_http_url(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
}

fn default_model_format() -> String {
    "bin".to_string()
}

fn installed_status_message(installed: &InstalledModel) -> &'static str {
    if installed.checksum_verified {
        "Installed and checksum verified."
    } else {
        "Installed. All model files are present."
    }
}

/// Files a faster-whisper model needs on disk to load fully offline.
/// `vocabulary.*` is `.txt` for small.en and `.json` for large-v3-turbo.
const FW_REQUIRED_FILES: [&str; 3] = ["model.bin", "config.json", "tokenizer.json"];

/// Check that a HuggingFace cache repo dir (`models--org--name`) holds a
/// complete faster-whisper snapshot: the revision `refs/main` points at, with
/// every required file present and non-empty. Returns the snapshot dir.
///
/// No sha256 pinning: these repos publish no hashes we control, and pinning a
/// hash without also pinning the HF revision would brick every install the
/// day the repo is updated. Integrity is HTTPS plus the worker successfully
/// loading the model.
fn verify_faster_whisper_snapshot(repo_dir: &Path) -> Result<PathBuf, String> {
    let revision = fs::read_to_string(repo_dir.join("refs").join("main"))
        .map_err(|err| format!("no downloaded revision ({err})"))?;
    let snapshot = repo_dir.join("snapshots").join(revision.trim());
    // fs::metadata follows symlinks, so a snapshot link into blobs/ is
    // measured by its real target size.
    let non_empty = |name: &str| {
        fs::metadata(snapshot.join(name))
            .map(|meta| meta.is_file() && meta.len() > 0)
            .unwrap_or(false)
    };
    if let Some(missing) = FW_REQUIRED_FILES.iter().find(|name| !non_empty(name)) {
        return Err(format!("{missing} is missing or empty"));
    }
    if !["vocabulary.txt", "vocabulary.json"]
        .iter()
        .any(|name| non_empty(name))
    {
        return Err("vocabulary file is missing or empty".to_string());
    }
    Ok(snapshot)
}

/// Display name and one-line performance note for a catalog row on this
/// machine (`hardware` = None: no machine-specific copy). Turbo is never
/// pitched as a good pick without a graphics card: on the CPU whisper pads
/// every clip to a 30 s window, so short dictations pay turbo's full encoder
/// cost (Ryzen 7 5800H: small.en ~2 s vs turbo 7-9 s per dictation).
/// `active`: the row is the model dictation uses now. `parakeet_on`: where
/// Parakeet runs, from its own GPU check (it has its own, separate from the
/// Whisper models'); `None` while that is unknown (not installed, check not
/// run yet or running).
fn model_fit(
    model_id: &str,
    hardware: Option<&HardwareProfile>,
    active: bool,
    parakeet_on: Option<AsrRung>,
) -> Option<(String, String)> {
    let tier = model_tier(model_id)?;
    let ggml = model_id.starts_with("wcpp-");
    let name = match tier {
        ModelTier::Small => "Small",
        ModelTier::LargeTurbo => "Large Turbo",
        ModelTier::Parakeet => "Parakeet v3",
    };
    let Some(hardware) = hardware else {
        return Some((name.to_string(), String::new()));
    };
    let turbo_fits = hardware.turbo_fits;
    let (suffix, note) = match (hardware.gpu_path(), ggml, tier) {
        // Its own GPU check decides, not the Whisper GPU path. Dictionary
        // words are boosted inside its decoder (plan 021).
        (_, _, ModelTier::Parakeet) => (
            "",
            match parakeet_on {
                Some(AsrRung::Gpu) => "NVIDIA's fast, accurate model, on your graphics card. Uses your dictionary words. English plus 24 European languages.",
                Some(AsrRung::Cpu) => "NVIDIA's fast, accurate model, on your processor. Uses your dictionary words. English plus 24 European languages.",
                None => "NVIDIA's fast, accurate model. Uses your dictionary words. English plus 24 European languages. Runs on your graphics card if it passes a quick check, otherwise on your processor.",
            },
        ),
        // GPU-path machine, its own rows.
        (true, true, ModelTier::Small) if !turbo_fits => ("", "Fast on your graphics card."),
        (true, true, ModelTier::Small) => (
            "",
            "Fast on your graphics card, at a third of the download.",
        ),
        (true, true, ModelTier::LargeTurbo) if turbo_fits => {
            ("", "High accuracy, and fast on your graphics card.")
        }
        (true, true, ModelTier::LargeTurbo) => (
            "",
            "High accuracy, but may be slow with this PC's graphics memory.",
        ),
        // GPU-path machine, an installed faster-whisper model: the one in
        // use (an upgrader before the switch), or kept as the fallback.
        (true, false, _) if active => (" (processor version)", "Runs on your processor."),
        (true, false, _) => (
            " (processor version)",
            "Kept as the backup if your graphics card has a problem. Remove it to free up space.",
        ),
        // CPU machine, its own rows.
        (false, false, ModelTier::Small) => (
            "",
            "Fast on any modern laptop: about 2 seconds per dictation.",
        ),
        (false, false, ModelTier::LargeTurbo) => (
            "",
            "High accuracy, but expect 7+ seconds per dictation without a graphics card.",
        ),
        // CPU machine, an installed GGML model (the GPU went away).
        (false, true, _) => (
            " (graphics card version)",
            "Made for a graphics card this PC is not using right now; slower on the processor.",
        ),
    };
    Some((format!("{name}{suffix}"), note.to_string()))
}

fn manifest_to_catalog_item(
    manifest: &SignedModelManifest,
    hardware: Option<&HardwareProfile>,
    active: bool,
    parakeet_on: Option<AsrRung>,
) -> Option<ModelCatalogItem> {
    let tier = model_tier(&manifest.model_id)?;
    let (display_name, hardware_note) =
        model_fit(&manifest.model_id, hardware, active, parakeet_on)?;
    Some(ModelCatalogItem {
        model_id: manifest.model_id.clone(),
        display_name,
        version: manifest.version.clone(),
        format: manifest.format.clone(),
        size_bytes: manifest.size,
        sha256: manifest.sha256.clone(),
        license: manifest.license.clone(),
        download_url: manifest.download_url.clone(),
        signature: manifest.signature.clone(),
        recommended: hardware
            .is_some_and(|hardware| manifest.model_id == hardware.recommended_model_id()),
        hardware_note,
        tier,
        primary: hardware.is_some_and(|hardware| hardware.model_id(tier) == manifest.model_id),
    })
}

fn manifest_signature_payload(manifest: &SignedModelManifest) -> String {
    format!(
        "model_id={}\\nversion={}\\nformat={}\\nsize={}\\nsha256={}\\nlicense={}\\ndownload_url={}\\n",
        manifest.model_id,
        manifest.version,
        manifest.format,
        manifest.size,
        manifest.sha256,
        manifest.license,
        manifest.download_url
    )
}

fn validate_manifest_signature(manifest: &SignedModelManifest) -> Result<(), ()> {
    let signature_bytes = base64::engine::general_purpose::STANDARD
        .decode(manifest.signature.as_bytes())
        .map_err(|_| ())?;
    let signature = Signature::from_slice(&signature_bytes).map_err(|_| ())?;
    let payload = manifest_signature_payload(manifest);
    manifest_verifying_key()
        .verify(payload.as_bytes(), &signature)
        .map_err(|_| ())
}

fn sign_manifest(manifest: &SignedModelManifest) -> String {
    let payload = manifest_signature_payload(manifest);
    let signature = manifest_signing_key()
        .expect("model manifest signing key is unavailable")
        .sign(payload.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(signature.to_bytes())
}

#[cfg(any(test, debug_assertions))]
fn manifest_signing_key() -> Option<SigningKey> {
    Some(SigningKey::from_bytes(&[
        0x53, 0x12, 0x29, 0x7A, 0x40, 0xCD, 0x81, 0x03, 0xE0, 0x11, 0xF1, 0x56, 0x0A, 0x44, 0xB8,
        0x92, 0xBC, 0x01, 0x77, 0xAB, 0x18, 0x6F, 0x99, 0x31, 0xD2, 0x2E, 0x50, 0x8D, 0x09, 0x6A,
        0x4F, 0xC1,
    ]))
}

#[cfg(not(any(test, debug_assertions)))]
fn manifest_signing_key() -> Option<SigningKey> {
    let encoded = env::var("VOICEWAVE_MANIFEST_SIGNING_KEY_B64").ok()?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim().as_bytes())
        .ok()?;
    if bytes.len() != 32 {
        return None;
    }
    let mut key = [0_u8; 32];
    key.copy_from_slice(&bytes);
    Some(SigningKey::from_bytes(&key))
}

fn manifest_verifying_key() -> VerifyingKey {
    let key_bytes = base64::engine::general_purpose::STANDARD
        .decode(MANIFEST_VERIFY_KEY_B64.as_bytes())
        .expect("manifest verify key must decode");
    let fixed: [u8; 32] = key_bytes
        .try_into()
        .expect("manifest verify key must be 32 bytes");
    VerifyingKey::from_bytes(&fixed).expect("manifest verify key must be valid")
}

fn build_catalog() -> Vec<SignedModelManifest> {
    if allow_synthetic_sources() {
        build_synthetic_catalog()
    } else {
        build_whispercpp_catalog()
    }
}

fn build_synthetic_catalog() -> Vec<SignedModelManifest> {
    let version = "phase3-local-synthetic-1";
    ["fw-small.en", "fw-large-v3-turbo"]
        .iter()
        .map(|model_id| {
            let is_fw = model_id.starts_with("fw-");
            let size = model_artifact_size_for(model_id);
            let payload = source_payload_for_manifest(model_id, version, size);
            let mut manifest = SignedModelManifest {
                model_id: (*model_id).to_string(),
                version: if is_fw {
                    FASTER_WHISPER_VERSION.to_string()
                } else {
                    version.to_string()
                },
                format: if is_fw {
                    FASTER_WHISPER_FORMAT.to_string()
                } else {
                    default_model_format()
                },
                size,
                sha256: sha256_hex(&payload),
                license: if is_fw {
                    "MIT (faster-whisper + model license)".to_string()
                } else {
                    "MIT-Local-Eval".to_string()
                },
                download_url: if *model_id == "fw-small.en" {
                    "faster-whisper://small.en".to_string()
                } else {
                    "faster-whisper://large-v3-turbo".to_string()
                },
                signature: String::new(),
            };
            manifest.signature = sign_manifest(&manifest);
            manifest
        })
        .collect()
}

fn build_whispercpp_catalog() -> Vec<SignedModelManifest> {
    let mut manifests = Vec::new();

    // Users see exactly two options: the fast default and the best-accuracy
    // model. fw-large-v3 was retired 2026-07; settings normalization migrates
    // users off it.
    let faster_rows = [
        (
            "fw-small.en",
            "small.en",
            487_614_201_u64,
            BUILTIN_FW_SMALL_SIGNATURE,
        ),
        (
            "fw-large-v3-turbo",
            "large-v3-turbo",
            1_620_000_000_u64,
            BUILTIN_FW_LARGE_TURBO_SIGNATURE,
        ),
    ];

    for (voicewave_id, runtime_id, size, signature) in faster_rows {
        let manifest = SignedModelManifest {
            model_id: voicewave_id.to_string(),
            version: FASTER_WHISPER_VERSION.to_string(),
            format: FASTER_WHISPER_FORMAT.to_string(),
            size,
            sha256: format!("{:064x}", size),
            license: "MIT (faster-whisper + model license)".to_string(),
            download_url: format!("faster-whisper://{runtime_id}"),
            signature: signature.to_string(),
        };
        manifests.push(manifest);
    }

    // Plan 019: whisper.cpp GGML models for the speech workers. Both families
    // are always in the catalog; `list_catalog` shows each machine its own.
    {
        let ggml_rows = [
            (
                "wcpp-small.en",
                "ggml-small.en.bin",
                487_614_201_u64,
                "c6138d6d58ecc8322097e0f987c32f1be8bb0a18532a3f88f734d1bbf9c41e5d",
                BUILTIN_WCPP_SMALL_EN_SIGNATURE,
            ),
            (
                "wcpp-large-v3-turbo",
                "ggml-large-v3-turbo.bin",
                1_624_555_275_u64,
                "1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69",
                BUILTIN_WCPP_LARGE_TURBO_SIGNATURE,
            ),
        ];
        for (model_id, file_name, size, sha256, signature) in ggml_rows {
            manifests.push(SignedModelManifest {
                model_id: model_id.to_string(),
                version: WCPP_VERSION.to_string(),
                format: WCPP_FORMAT.to_string(),
                size,
                sha256: sha256.to_string(),
                license: "MIT (whisper.cpp + OpenAI Whisper weights)".to_string(),
                download_url: format!(
                    "https://huggingface.co/ggerganov/whisper.cpp/resolve/{GGML_REPO_REVISION}/{file_name}"
                ),
                signature: signature.to_string(),
            });
        }
    }

    // NVIDIA Parakeet TDT 0.6B v3 for the same speech workers (parakeet.h in
    // the pinned whisper.cpp tree). q8_0: about half the f16 download.
    manifests.push(SignedModelManifest {
        model_id: PARAKEET_MODEL_ID.to_string(),
        version: "parakeet-gguf-3515645".to_string(),
        format: WCPP_FORMAT.to_string(),
        size: 668_757_119,
        sha256: "4d64e9e96c2792186d072fde0034df0ad670cf680a2f53069052ead827fd600e".to_string(),
        license: "CC-BY-4.0 (NVIDIA Parakeet TDT 0.6B v3; GGML conversion by ggml-org)".to_string(),
        download_url: format!(
            "https://huggingface.co/ggml-org/parakeet-GGUF/resolve/{PARAKEET_REPO_REVISION}/ggml-parakeet-tdt-0.6b-v3-q8_0.bin"
        ),
        signature: BUILTIN_WCPP_PARAKEET_V3_SIGNATURE.to_string(),
    });

    manifests
}

fn model_artifact_size_for(model_id: &str) -> u64 {
    match model_id {
        "fw-small.en" => 1024 * 1024,
        "fw-large-v3-turbo" => 2 * 1024 * 1024,
        _ => 384 * 1024,
    }
}

fn source_payload_for_manifest(model_id: &str, version: &str, expected_size: u64) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(expected_size as usize);
    // Prefix deterministic payload with GGUF magic bytes so artifacts are format-labeled.
    bytes.extend_from_slice(b"GGUF");
    bytes.extend_from_slice(&[0, 0, 0, 0]);
    let seed = format!("voicewave-model-artifact::{model_id}::{version}::");

    while bytes.len() < expected_size as usize {
        bytes.extend_from_slice(seed.as_bytes());
        bytes.extend_from_slice(format!("{:08x}\\n", bytes.len()).as_bytes());
    }

    bytes.truncate(expected_size as usize);
    bytes
}

fn sha256_hex(bytes: &[u8]) -> String {
    to_hex(&Sha256::digest(bytes))
}

/// SHA-256 of a file, streamed so a multi-GB model never sits in RAM.
fn sha256_file(path: &Path) -> io::Result<String> {
    let mut hasher = Sha256::new();
    io::copy(&mut fs::File::open(path)?, &mut hasher)?;
    Ok(to_hex(&hasher.finalize()))
}

fn to_hex(digest: &[u8]) -> String {
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // Run with `cargo test generate_fw_small_catalog_signature -- --nocapture` to
    // get the hardcoded constant value for BUILTIN_FW_SMALL_SIGNATURE.
    #[test]
    fn generate_fw_small_catalog_signature() {
        let mut manifest = SignedModelManifest {
            model_id: "fw-small.en".to_string(),
            version: FASTER_WHISPER_VERSION.to_string(),
            format: FASTER_WHISPER_FORMAT.to_string(),
            size: 487_614_201_u64,
            sha256: format!("{:064x}", 487_614_201_u64),
            license: "MIT (faster-whisper + model license)".to_string(),
            download_url: "faster-whisper://small.en".to_string(),
            signature: String::new(),
        };
        manifest.signature = sign_manifest(&manifest);
        println!("// fw-small.en");
        println!("const SIG: &str = \"{}\";", manifest.signature);
        assert!(
            validate_manifest_signature(&manifest).is_ok(),
            "generated signature for fw-small.en should be valid"
        );
    }

    // Run with `cargo test generate_fw_turbo_catalog_signature -- --nocapture` to
    // get the hardcoded constant value for BUILTIN_FW_LARGE_TURBO_SIGNATURE.
    #[test]
    fn generate_fw_turbo_catalog_signature() {
        let mut manifest = SignedModelManifest {
            model_id: "fw-large-v3-turbo".to_string(),
            version: FASTER_WHISPER_VERSION.to_string(),
            format: FASTER_WHISPER_FORMAT.to_string(),
            size: 1_620_000_000_u64,
            sha256: format!("{:064x}", 1_620_000_000_u64),
            license: "MIT (faster-whisper + model license)".to_string(),
            download_url: "faster-whisper://large-v3-turbo".to_string(),
            signature: String::new(),
        };
        manifest.signature = sign_manifest(&manifest);
        println!("// fw-large-v3-turbo");
        println!("const SIG: &str = \"{}\";", manifest.signature);
        assert!(
            validate_manifest_signature(&manifest).is_ok(),
            "generated signature for fw-large-v3-turbo should be valid"
        );
    }

    #[test]
    fn fw_catalog_includes_large_v3_turbo_with_valid_signature() {
        let catalog = build_whispercpp_catalog();
        let manifest = catalog
            .iter()
            .find(|m| m.model_id == "fw-large-v3-turbo")
            .expect("catalog should include fw-large-v3-turbo");
        assert_eq!(manifest.download_url, "faster-whisper://large-v3-turbo");
        assert!(
            validate_manifest_signature(manifest).is_ok(),
            "fw-large-v3-turbo manifest should have a valid signature"
        );
    }

    #[test]
    fn catalog_holds_exactly_the_two_supported_models() {
        let catalog = build_whispercpp_catalog();
        let ids: Vec<&str> = catalog.iter().map(|m| m.model_id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "fw-small.en",
                "fw-large-v3-turbo",
                "wcpp-small.en",
                "wcpp-large-v3-turbo",
                "wcpp-parakeet-tdt-0.6b-v3"
            ],
            "fw-large-v3 is retired; both families stay in the catalog (plan 019)"
        );
        for manifest in &catalog {
            assert!(
                validate_manifest_signature(manifest).is_ok(),
                "manifest '{}' should have a valid signature",
                manifest.model_id
            );
        }
    }

    fn test_root(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("voicewave-model-tests-{name}-{}", now_utc_ms()));
        fs::create_dir_all(&root).expect("create test root");
        root
    }

    fn configure_manifest_for_file_download(manager: &mut ModelManager, model_id: &str) -> PathBuf {
        let manifest = manager
            .catalog
            .iter_mut()
            .find(|row| row.model_id == model_id)
            .expect("manifest should exist");

        fs::create_dir_all(&manager.source_dir).expect("create source dir");
        let source_path = manager
            .source_dir
            .join(format!("{}-test-source.bin", model_id.replace('.', "_")));
        let payload =
            source_payload_for_manifest(&manifest.model_id, &manifest.version, manifest.size);
        fs::write(&source_path, payload).expect("seed local source payload");
        let payload = fs::read(&source_path).expect("read local source payload");
        manifest.size = payload.len() as u64;
        manifest.sha256 = sha256_hex(&payload);

        manifest.download_url = format!("file://{}", source_path.to_string_lossy());
        manifest.signature = sign_manifest(manifest);
        source_path
    }

    #[test]
    fn tampered_model_is_rejected_and_quarantined() {
        let root = test_root("tampered");
        let mut manager = ModelManager::with_test_paths(&root).expect("manager");
        let source_path = configure_manifest_for_file_download(&mut manager, "fw-small.en");

        let manifest = manager
            .catalog
            .iter()
            .find(|row| row.model_id == "fw-small.en")
            .cloned()
            .expect("manifest");
        let resolved_source = manager
            .resolve_download_source(&manifest)
            .expect("source path");
        assert_eq!(resolved_source, source_path);

        let mut tampered = fs::read(&source_path).expect("source bytes");
        tampered[0] ^= 0xFF;
        fs::write(&source_path, tampered).expect("write tampered source");

        let err = manager
            .install_model_resumable("fw-small.en", || false, || false, |_| {})
            .expect_err("install should fail");

        match err {
            ModelError::ChecksumMismatch { .. } => {}
            other => panic!("expected checksum mismatch, got {other:?}"),
        }

        let quarantined = fs::read_dir(manager.quarantine_dir.clone())
            .expect("quarantine dir")
            .filter_map(Result::ok)
            .count();
        assert!(quarantined > 0, "tampered artifact should be quarantined");

        let status = manager
            .get_download_status("fw-small.en", None)
            .expect("failed status");
        assert_eq!(status.state, ModelStatusState::Failed);
        assert!(status.resumable);
    }

    #[test]
    fn interrupted_download_resumes_from_checkpoint() {
        let root = test_root("resume");
        let mut manager = ModelManager::with_test_paths(&root).expect("manager");
        configure_manifest_for_file_download(&mut manager, "fw-small.en");
        manager.set_test_chunk_size(2048);

        let mut pause_after_checks = 0_u32;
        let paused = manager
            .install_model_resumable(
                "fw-small.en",
                || false,
                || {
                    pause_after_checks += 1;
                    pause_after_checks > 6
                },
                |_| {},
            )
            .expect("pause result");

        assert_eq!(paused.state, ModelStatusState::Paused);
        assert!(paused.progress > 0);
        assert!(paused.progress < 100);

        let installed = manager
            .install_model_resumable("fw-small.en", || false, || false, |_| {})
            .expect("resume to completion");

        assert_eq!(installed.state, ModelStatusState::Installed);
        assert!(installed.installed);
        assert_eq!(installed.progress, 100);
    }

    #[test]
    fn low_disk_failure_can_retry_after_capacity_restored() {
        let root = test_root("lowdisk");
        let mut manager = ModelManager::with_test_paths(&root).expect("manager");
        configure_manifest_for_file_download(&mut manager, "fw-small.en");
        manager.set_test_storage_limit(Some(16 * 1024));

        let err = manager
            .install_model_resumable("fw-small.en", || false, || false, |_| {})
            .expect_err("low disk should fail");

        match err {
            ModelError::LowDisk { .. } => {}
            other => panic!("expected low disk error, got {other:?}"),
        }

        manager.set_test_storage_limit(None);
        let installed = manager
            .install_model_resumable("fw-small.en", || false, || false, |_| {})
            .expect("retry should succeed");
        assert_eq!(installed.state, ModelStatusState::Installed);
    }

    #[test]
    fn cancel_then_retry_completes_install() {
        let root = test_root("cancel-retry");
        let mut manager = ModelManager::with_test_paths(&root).expect("manager");
        configure_manifest_for_file_download(&mut manager, "fw-large-v3-turbo");
        manager.set_test_chunk_size(2048);

        let mut checks = 0_u32;
        let cancelled = manager
            .install_model_resumable(
                "fw-large-v3-turbo",
                || {
                    checks += 1;
                    checks > 4
                },
                || false,
                |_| {},
            )
            .expect("cancel status");

        assert_eq!(cancelled.state, ModelStatusState::Cancelled);
        assert!(cancelled.resumable);

        let installed = manager
            .install_model_resumable("fw-large-v3-turbo", || false, || false, |_| {})
            .expect("retry after cancel should install");
        assert_eq!(installed.state, ModelStatusState::Installed);
    }

    #[test]
    fn invalid_manifest_signature_rejects_and_quarantines_partial() {
        let root = test_root("signature");
        let mut manager = ModelManager::with_test_paths(&root).expect("manager");
        configure_manifest_for_file_download(&mut manager, "fw-small.en");

        let manifest = manager
            .catalog
            .iter()
            .find(|row| row.model_id == "fw-small.en")
            .cloned()
            .expect("manifest");
        let partial_path = manager.partial_model_path_for(&manifest);
        fs::create_dir_all(partial_path.parent().expect("partial parent"))
            .expect("create partial parent");
        fs::write(&partial_path, b"partial-tampered").expect("seed partial");

        let tampered_manifest = manager
            .catalog
            .iter_mut()
            .find(|row| row.model_id == "fw-small.en")
            .expect("tampered manifest");
        tampered_manifest.signature = "broken-signature".to_string();

        let err = manager
            .install_model_resumable("fw-small.en", || false, || false, |_| {})
            .expect_err("signature must fail");

        match err {
            ModelError::ManifestSignatureInvalid { .. } => {}
            other => panic!("expected signature failure, got {other:?}"),
        }

        assert!(
            !partial_path.exists(),
            "partial should be quarantined/removed"
        );
    }

    #[test]
    fn corrupt_partial_is_quarantined_then_recovered() {
        let root = test_root("corrupt-partial");
        let mut manager = ModelManager::with_test_paths(&root).expect("manager");
        configure_manifest_for_file_download(&mut manager, "fw-small.en");

        let manifest = manager
            .catalog
            .iter()
            .find(|row| row.model_id == "fw-small.en")
            .cloned()
            .expect("manifest");
        let partial_path = manager.partial_model_path_for(&manifest);
        fs::create_dir_all(partial_path.parent().expect("partial parent"))
            .expect("create partial parent");
        fs::write(&partial_path, vec![0_u8; manifest.size as usize + 4096])
            .expect("oversized partial");

        let installed = manager
            .install_model_resumable("fw-small.en", || false, || false, |_| {})
            .expect("should recover from corrupt partial");
        assert_eq!(installed.state, ModelStatusState::Installed);
    }

    #[test]
    fn load_installed_recovers_from_blank_file() {
        let root = test_root("blank-installed-index");
        let mut manager = ModelManager::with_test_paths(&root).expect("manager");
        fs::write(&manager.installed_index_path, "\u{feff}   \n  ").expect("seed blank metadata");

        manager
            .load_installed()
            .expect("blank installed metadata should recover");
        assert!(manager.installed.is_empty());

        let rewritten =
            fs::read_to_string(&manager.installed_index_path).expect("rewritten metadata");
        assert!(
            rewritten.contains("\"installed\""),
            "installed metadata should be rewritten with default structure"
        );
    }

    /// A zero-filled index is the unclean-shutdown signature; `trim()` alone
    /// would let those NUL bytes through to serde.
    #[test]
    fn load_installed_recovers_from_zero_filled_file() {
        let root = test_root("zero-filled-installed-index");
        let mut manager = ModelManager::with_test_paths(&root).expect("manager");
        fs::write(&manager.installed_index_path, vec![0_u8; 299]).expect("seed zero-filled index");

        manager
            .load_installed()
            .expect("zero-filled installed metadata should recover");
        assert!(manager.installed.is_empty());

        let rewritten =
            fs::read_to_string(&manager.installed_index_path).expect("rewritten metadata");
        assert!(rewritten.contains("\"installed\""));
    }

    #[test]
    fn load_downloads_recovers_from_corrupt_json_file() {
        let root = test_root("corrupt-download-index");
        let mut manager = ModelManager::with_test_paths(&root).expect("manager");
        fs::write(&manager.download_state_path, "{not-valid-json").expect("seed corrupt metadata");

        manager
            .load_downloads()
            .expect("corrupt download metadata should recover");
        assert!(manager.downloads.is_empty());

        let rewritten =
            fs::read_to_string(&manager.download_state_path).expect("rewritten downloads metadata");
        assert!(
            rewritten.contains("\"downloads\""),
            "download metadata should be rewritten with default structure"
        );
    }
}
