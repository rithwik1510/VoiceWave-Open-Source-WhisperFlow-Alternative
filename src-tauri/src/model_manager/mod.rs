use crate::atomic_file;
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
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const FASTER_WHISPER_FORMAT: &str = "faster-whisper";
const FASTER_WHISPER_VERSION: &str = "faster-whisper-v1";
const WCPP_FORMAT: &str = "ggml";
const MANIFEST_VERIFY_KEY_B64: &str = "BFHdCqQYd/56J4tm8cYMUnhrfHOR6YyJqiwXPgO+5gU=";
// Generated via `generate_fw_small_catalog_signature` test (dev key = production key).
const BUILTIN_FW_SMALL_SIGNATURE: &str =
    "6pLuyp8Z495RkvKNs9MSnDu8DvG6i+kd8YHtWfQReMKNAB8DVrN6fa9I695K7R+XvxnumprISe1cOQS089cKAA==";
// Generated via `generate_fw_turbo_catalog_signature` test (dev key = production key).
const BUILTIN_FW_LARGE_TURBO_SIGNATURE: &str =
    "IfGO/sqcNE4flIvVG/YcTlrx2DbUDntDGHUtG1dWTiiwEwqwZbYZO/pCwf0q8YZ0gMRmHYyQLddf3Es+Ux42DQ==";

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
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("unknown model id: {0}")]
    UnknownModel(String),
    #[error("failed to read model metadata: {0}")]
    Read(std::io::Error),
    #[error("failed to write model metadata: {0}")]
    Write(std::io::Error),
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
    updated_at_utc_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct DownloadCheckpointStore {
    downloads: Vec<DownloadCheckpoint>,
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

    pub fn list_catalog(&self) -> Vec<ModelCatalogItem> {
        self.catalog.iter().map(manifest_to_catalog_item).collect()
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

        let size_bytes = directory_size_bytes(&cache_path).unwrap_or(manifest.size);
        let installed = InstalledModel {
            model_id: manifest.model_id.clone(),
            version: manifest.version.clone(),
            format: manifest.format.clone(),
            size_bytes,
            file_path: cache_path.to_string_lossy().to_string(),
            sha256: manifest.sha256.clone(),
            installed_at_utc_ms: now_utc_ms(),
            checksum_verified: true,
        };
        self.installed
            .insert(installed.model_id.clone(), installed.clone());
        self.persist_installed()?;
        Ok(installed)
    }

    pub fn get_catalog_item(&self, model_id: &str) -> Option<ModelCatalogItem> {
        self.catalog
            .iter()
            .find(|item| item.model_id == model_id)
            .map(manifest_to_catalog_item)
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
                message: Some("Installed and checksum verified.".to_string()),
                installed_model: Some(installed),
                downloaded_bytes: None,
                total_bytes: None,
                resumable: false,
            });
        }

        if let Some(checkpoint) = self.downloads.get(model_id) {
            return Some(self.status_from_checkpoint(
                model_id,
                checkpoint,
                active_model.is_some_and(|value| value == model_id),
                None,
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
        })
    }

    pub fn install_model_resumable<F, C, P>(
        &mut self,
        model_id: &str,
        mut should_cancel: C,
        mut should_pause: P,
        mut on_progress: F,
    ) -> Result<ModelStatus, ModelError>
    where
        F: FnMut(ModelStatus),
        C: FnMut() -> bool,
        P: FnMut() -> bool,
    {
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

        if validate_manifest_signature(&manifest).is_err() {
            if let Some(checkpoint) = self.downloads.remove(model_id) {
                let _ = self.quarantine_path(
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
                let _ = self.quarantine_path(&orphan_partial, model_id, "manifest-signature");
                let _ = fs::remove_file(&orphan_partial);
            }
            return Err(ModelError::ManifestSignatureInvalid {
                model_id: model_id.to_string(),
            });
        }

        fs::create_dir_all(self.model_dir.join("downloads")).map_err(ModelError::Write)?;
        fs::create_dir_all(&self.quarantine_dir).map_err(ModelError::Write)?;

        let mut checkpoint = self.prepare_checkpoint(&manifest)?;

        if should_cancel() {
            checkpoint.state = ModelStatusState::Cancelled;
            checkpoint.last_error = Some("Download cancelled before start.".to_string());
            checkpoint.updated_at_utc_ms = now_utc_ms();
            self.downloads
                .insert(model_id.to_string(), checkpoint.clone());
            self.persist_downloads()?;
            let status = self.status_from_checkpoint(model_id, &checkpoint, false, None);
            on_progress(status.clone());
            return Ok(status);
        }

        let download_url = manifest.download_url.clone();
        if is_http_url(&download_url) && checkpoint.downloaded_bytes > 0 && !http_resume_enabled() {
            if let Ok(_) = fs::remove_file(&checkpoint.partial_path) {
                checkpoint.downloaded_bytes = 0;
                checkpoint.state = ModelStatusState::Downloading;
                checkpoint.last_error =
                    Some("Restarting download to avoid corrupted resume data.".to_string());
                checkpoint.updated_at_utc_ms = now_utc_ms();
                self.downloads
                    .insert(model_id.to_string(), checkpoint.clone());
                self.persist_downloads()?;
            }
        }

        let mut partial_file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&checkpoint.partial_path)
            .map_err(ModelError::Write)?;

        if checkpoint.downloaded_bytes == 0 {
            let _ = partial_file.set_len(0);
        }

        let mut source: Box<dyn Read> = if is_http_url(&download_url) {
            let agent = ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(15))
                .timeout_read(Duration::from_secs(30))
                .build();
            let mut request = agent.get(&download_url).set("User-Agent", "voicewave/0.1");
            if checkpoint.downloaded_bytes > 0 && http_resume_enabled() {
                request = request.set("Range", &format!("bytes={}-", checkpoint.downloaded_bytes));
            }
            let mut response = request.call().map_err(|err| {
                ModelError::Read(io::Error::new(io::ErrorKind::Other, err.to_string()))
            })?;
            if checkpoint.downloaded_bytes > 0 && http_resume_enabled() && response.status() != 206
            {
                checkpoint.downloaded_bytes = 0;
                checkpoint.state = ModelStatusState::Downloading;
                checkpoint.last_error =
                    Some("Source does not support resume; restarted download.".to_string());
                checkpoint.updated_at_utc_ms = now_utc_ms();
                let _ = partial_file.set_len(0);
                self.downloads
                    .insert(model_id.to_string(), checkpoint.clone());
                self.persist_downloads()?;
                response = agent
                    .get(&download_url)
                    .set("User-Agent", "voicewave/0.1")
                    .call()
                    .map_err(|err| {
                        ModelError::Read(io::Error::new(io::ErrorKind::Other, err.to_string()))
                    })?;
            }
            Box::new(response.into_reader())
        } else {
            let source_path = self.resolve_download_source(&manifest)?;
            let source_meta = fs::metadata(&source_path).map_err(|err| {
                if err.kind() == io::ErrorKind::NotFound {
                    ModelError::SourceMissing {
                        model_id: model_id.to_string(),
                        expected_path: source_path.to_string_lossy().to_string(),
                    }
                } else {
                    ModelError::Read(err)
                }
            })?;
            if source_meta.len() != manifest.size {
                return Err(ModelError::ManifestSizeMismatch {
                    model_id: model_id.to_string(),
                    expected: manifest.size,
                    actual: source_meta.len(),
                });
            }
            let mut file = fs::File::open(&source_path).map_err(|err| {
                if err.kind() == io::ErrorKind::NotFound {
                    ModelError::SourceMissing {
                        model_id: model_id.to_string(),
                        expected_path: source_path.to_string_lossy().to_string(),
                    }
                } else {
                    ModelError::Read(err)
                }
            })?;
            file.seek(SeekFrom::Start(checkpoint.downloaded_bytes))
                .map_err(ModelError::Read)?;
            Box::new(file)
        };

        let mut buffer = vec![0_u8; self.download_chunk_bytes.max(4 * 1024)];

        while checkpoint.downloaded_bytes < checkpoint.total_bytes {
            if should_cancel() {
                checkpoint.state = ModelStatusState::Cancelled;
                checkpoint.last_error = Some("Download cancelled by user.".to_string());
                checkpoint.updated_at_utc_ms = now_utc_ms();
                self.downloads
                    .insert(model_id.to_string(), checkpoint.clone());
                self.persist_downloads()?;
                let status = self.status_from_checkpoint(model_id, &checkpoint, false, None);
                on_progress(status.clone());
                return Ok(status);
            }
            if should_pause() {
                checkpoint.state = ModelStatusState::Paused;
                checkpoint.last_error = Some("Download paused.".to_string());
                checkpoint.updated_at_utc_ms = now_utc_ms();
                self.downloads
                    .insert(model_id.to_string(), checkpoint.clone());
                self.persist_downloads()?;
                let status = self.status_from_checkpoint(model_id, &checkpoint, false, None);
                on_progress(status.clone());
                return Ok(status);
            }

            let read = source.read(&mut buffer).map_err(ModelError::Read)?;
            if read == 0 {
                checkpoint.state = ModelStatusState::Failed;
                checkpoint.last_error =
                    Some("Source stream ended before expected size.".to_string());
                checkpoint.updated_at_utc_ms = now_utc_ms();
                self.downloads
                    .insert(model_id.to_string(), checkpoint.clone());
                self.persist_downloads()?;
                return Err(ModelError::CorruptSource {
                    model_id: model_id.to_string(),
                    expected: checkpoint.total_bytes,
                });
            }

            let next_total = checkpoint.downloaded_bytes + read as u64;
            if let Some(limit) = self.max_storage_bytes {
                if next_total > limit {
                    checkpoint.state = ModelStatusState::Failed;
                    checkpoint.last_error = Some(format!(
                        "Low disk detected. Needed {next_total} bytes but limit is {limit}."
                    ));
                    checkpoint.updated_at_utc_ms = now_utc_ms();
                    self.downloads
                        .insert(model_id.to_string(), checkpoint.clone());
                    self.persist_downloads()?;
                    let status = self.status_from_checkpoint(model_id, &checkpoint, false, None);
                    on_progress(status.clone());
                    return Err(ModelError::LowDisk {
                        model_id: model_id.to_string(),
                        limit,
                    });
                }
            }

            if let Err(write_err) = partial_file.write_all(&buffer[..read]) {
                let kind = write_err.kind();
                if kind == io::ErrorKind::StorageFull || write_err.raw_os_error() == Some(112) {
                    checkpoint.state = ModelStatusState::Failed;
                    checkpoint.last_error =
                        Some("Low disk while writing model artifact.".to_string());
                    checkpoint.updated_at_utc_ms = now_utc_ms();
                    self.downloads
                        .insert(model_id.to_string(), checkpoint.clone());
                    self.persist_downloads()?;
                    return Err(ModelError::LowDisk {
                        model_id: model_id.to_string(),
                        limit: checkpoint.downloaded_bytes + read as u64,
                    });
                }
                return Err(ModelError::Write(write_err));
            }

            checkpoint.downloaded_bytes = next_total;
            checkpoint.state = ModelStatusState::Downloading;
            checkpoint.last_error = None;
            checkpoint.updated_at_utc_ms = now_utc_ms();

            self.downloads
                .insert(model_id.to_string(), checkpoint.clone());
            self.persist_downloads()?;
            let status = self.status_from_checkpoint(model_id, &checkpoint, false, None);
            on_progress(status.clone());
        }

        partial_file.flush().map_err(ModelError::Write)?;

        let final_path = self.final_model_path_for(&manifest);
        if final_path.exists() {
            fs::remove_file(&final_path).map_err(ModelError::Write)?;
        }
        fs::rename(&checkpoint.partial_path, &final_path).map_err(ModelError::Write)?;

        // GGML/GGUF downloads trust HTTPS integrity (no pre-computed hash available).
        // Skip reading the whole file into RAM — use metadata for size instead.
        let skip_checksum = manifest.format == WCPP_FORMAT;
        let (final_size, verified_checksum) = if skip_checksum {
            let size = fs::metadata(&final_path)
                .map(|m| m.len())
                .unwrap_or(manifest.size);
            (size, String::new())
        } else {
            let verification_bytes = fs::read(&final_path).map_err(ModelError::Read)?;
            let checksum = sha256_hex(&verification_bytes);
            (verification_bytes.len() as u64, checksum)
        };

        if !skip_checksum && verified_checksum != manifest.sha256 {
            let quarantine_path = self
                .quarantine_path(&final_path, model_id, "checksum-mismatch")?
                .map(|path| path.to_string_lossy().to_string());
            checkpoint.downloaded_bytes = 0;
            checkpoint.state = ModelStatusState::Failed;
            checkpoint.last_error = Some(format!(
                "Checksum mismatch (expected {}, got {}). Artifact quarantined.",
                manifest.sha256, verified_checksum
            ));
            checkpoint.updated_at_utc_ms = now_utc_ms();
            self.downloads
                .insert(model_id.to_string(), checkpoint.clone());
            self.persist_downloads()?;
            return Err(ModelError::ChecksumMismatch {
                model_id: model_id.to_string(),
                expected: manifest.sha256,
                actual: verified_checksum,
                quarantine_path,
            });
        }

        let installed = InstalledModel {
            model_id: manifest.model_id.clone(),
            version: manifest.version.clone(),
            format: manifest.format.clone(),
            size_bytes: final_size,
            file_path: final_path.to_string_lossy().to_string(),
            sha256: verified_checksum,
            installed_at_utc_ms: now_utc_ms(),
            checksum_verified: !skip_checksum,
        };

        self.installed
            .insert(installed.model_id.clone(), installed.clone());
        self.persist_installed()?;
        self.downloads.remove(model_id);
        self.persist_downloads()?;

        let status = ModelStatus {
            model_id: model_id.to_string(),
            state: ModelStatusState::Installed,
            progress: 100,
            active: false,
            installed: true,
            message: Some(
                "Model installed successfully with manifest + checksum verification.".to_string(),
            ),
            installed_model: Some(installed),
            downloaded_bytes: Some(manifest.size),
            total_bytes: Some(manifest.size),
            resumable: false,
        };
        on_progress(status.clone());
        Ok(status)
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
                    updated_at_utc_ms: now_utc_ms(),
                });

        if checkpoint.version != manifest.version {
            let old_path = PathBuf::from(&checkpoint.partial_path);
            let _ = self.quarantine_path(&old_path, &manifest.model_id, "version-drift");
            let _ = fs::remove_file(old_path);
            checkpoint.version = manifest.version.clone();
            checkpoint.partial_path = partial_path.to_string_lossy().to_string();
            checkpoint.downloaded_bytes = 0;
            checkpoint.state = ModelStatusState::Idle;
            checkpoint.last_error = None;
        }

        if partial_path.exists() {
            let file_size = fs::metadata(&partial_path).map_err(ModelError::Read)?.len();
            if file_size > manifest.size {
                let _ =
                    self.quarantine_path(&partial_path, &manifest.model_id, "oversized-partial");
                let _ = fs::remove_file(&partial_path);
                checkpoint.downloaded_bytes = 0;
                checkpoint.state = ModelStatusState::Failed;
                checkpoint.last_error =
                    Some("Corrupt partial artifact detected and quarantined.".to_string());
            } else {
                checkpoint.downloaded_bytes = file_size;
            }
        } else {
            checkpoint.downloaded_bytes = 0;
        }

        checkpoint.total_bytes = manifest.size;
        checkpoint.updated_at_utc_ms = now_utc_ms();

        self.downloads
            .insert(manifest.model_id.clone(), checkpoint.clone());
        self.persist_downloads()?;
        Ok(checkpoint)
    }

    fn status_from_checkpoint(
        &self,
        model_id: &str,
        checkpoint: &DownloadCheckpoint,
        active: bool,
        message_override: Option<String>,
    ) -> ModelStatus {
        let progress = if checkpoint.total_bytes == 0 {
            0
        } else {
            ((checkpoint.downloaded_bytes.saturating_mul(100)) / checkpoint.total_bytes).min(100)
                as u8
        };

        let resumable = matches!(
            checkpoint.state,
            ModelStatusState::Downloading
                | ModelStatusState::Paused
                | ModelStatusState::Cancelled
                | ModelStatusState::Failed
        );

        ModelStatus {
            model_id: model_id.to_string(),
            state: checkpoint.state.clone(),
            progress,
            active,
            installed: false,
            message: message_override
                .or_else(|| checkpoint.last_error.clone())
                .or_else(|| Some("Model download checkpoint restored.".to_string())),
            installed_model: None,
            downloaded_bytes: Some(checkpoint.downloaded_bytes),
            total_bytes: Some(checkpoint.total_bytes),
            resumable,
        }
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
                eprintln!(
                    "voicewave: failed to read the installed model index at '{}': {}",
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
                eprintln!(
                    "voicewave: corrupt installed model index recovered at '{}': {}",
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
                if !Path::new(&model.file_path).exists() {
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
                        let _ = self.quarantine_path(path, model_id, "size-mismatch");
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
                eprintln!(
                    "voicewave: failed to read the model download checkpoints at '{}': {}",
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
                eprintln!(
                    "voicewave: corrupt model download checkpoint recovered at '{}': {}",
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
                let payload = fs::read(&source_path).map_err(ModelError::Read)?;
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
                manifest.size = payload.len() as u64;
                manifest.sha256 = sha256_hex(&payload);
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

    fn quarantine_path(
        &self,
        path: &Path,
        model_id: &str,
        reason: &str,
    ) -> Result<Option<PathBuf>, ModelError> {
        if !path.exists() {
            return Ok(None);
        }
        fs::create_dir_all(&self.quarantine_dir).map_err(ModelError::Write)?;

        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("bin");
        let target = self.quarantine_dir.join(format!(
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
            let _ = self.quarantine_path(&partial_path, &model_id, "oversized-partial");
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
        let metadata = fs::metadata(&entry_path).ok()?;
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

fn http_resume_enabled() -> bool {
    env::var("VOICEWAVE_HTTP_RESUME")
        .map(|value| value == "1")
        .unwrap_or(false)
}

fn default_model_format() -> String {
    "bin".to_string()
}

fn manifest_to_catalog_item(manifest: &SignedModelManifest) -> ModelCatalogItem {
    let display_name = match manifest.model_id.as_str() {
        "fw-small.en" => "faster-whisper small.en".to_string(),
        "fw-large-v3-turbo" => {
            "faster-whisper large-v3-turbo (~1.6 GB, best accuracy on GPU)".to_string()
        }
        _ => manifest.model_id.clone(),
    };
    ModelCatalogItem {
        model_id: manifest.model_id.clone(),
        display_name,
        version: manifest.version.clone(),
        format: manifest.format.clone(),
        size_bytes: manifest.size,
        sha256: manifest.sha256.clone(),
        license: manifest.license.clone(),
        download_url: manifest.download_url.clone(),
        signature: manifest.signature.clone(),
    }
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
        0x53, 0x12, 0x29, 0x7A, 0x40, 0xCD, 0x81, 0x03, 0xE0, 0x11, 0xF1, 0x56, 0x0A, 0x44,
        0xB8, 0x92, 0xBC, 0x01, 0x77, 0xAB, 0x18, 0x6F, 0x99, 0x31, 0xD2, 0x2E, 0x50, 0x8D,
        0x09, 0x6A, 0x4F, 0xC1,
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

    // Catalog intentionally holds exactly two options: the fast default and the
    // best-accuracy GPU model. fw-large-v3 and the wcpp-* GGML entries were
    // retired 2026-07; settings normalization migrates users off them.
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
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
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
            vec!["fw-small.en", "fw-large-v3-turbo"],
            "retired models (fw-large-v3, wcpp-*) must not reappear in the catalog"
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
