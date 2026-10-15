use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};

const RUNTIME_VERSION: &str = "cuda12-2026.09.1";
const CHUNK_SIZE: usize = 1024 * 1024;

struct Artifact {
    filename: &'static str,
    url: &'static str,
    sha256: &'static str,
    size: u64,
}

const ARTIFACTS: &[Artifact] = &[
    Artifact {
        filename: "nvidia_cublas_cu12-12.9.2.10-py3-none-win_amd64.whl",
        url: "https://files.pythonhosted.org/packages/20/e2/fc9a0e985249d873150276d5afb02e39a66817fedbf1a385724393e505ed/nvidia_cublas_cu12-12.9.2.10-py3-none-win_amd64.whl",
        sha256: "623f43027d40d44ceadf0043f002bd25cf353e8f13ce90b9a87057019f560661",
        size: 553_162_896,
    },
    Artifact {
        filename: "nvidia_cudnn_cu12-9.26.0.51-py3-none-win_amd64.whl",
        url: "https://files.pythonhosted.org/packages/c5/ee/baebebf270df5a57830e40879b4016de47ca43961095cf18b7749452150f/nvidia_cudnn_cu12-9.26.0.51-py3-none-win_amd64.whl",
        sha256: "010abb90f513fc6e2b6e9278d56f594b87922fff80737434ab415da609db8311",
        size: 746_465_599,
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GpuRuntimeState {
    NotInstalled,
    Downloading,
    Verifying,
    Installing,
    Ready,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuRuntimeStatus {
    pub state: GpuRuntimeState,
    pub installed_version: Option<String>,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub percent: u8,
    pub message: String,
    pub restart_required: bool,
    pub cuda_verified: bool,
}

impl Default for GpuRuntimeStatus {
    fn default() -> Self {
        Self {
            state: GpuRuntimeState::NotInstalled,
            installed_version: None,
            downloaded_bytes: 0,
            total_bytes: ARTIFACTS.iter().map(|item| item.size).sum(),
            percent: 0,
            message: "GPU runtime is not installed".to_string(),
            restart_required: false,
            cuda_verified: false,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct InstalledManifest {
    version: String,
    artifacts: Vec<String>,
    cuda_verified: bool,
}

static STATUS: Mutex<Option<GpuRuntimeStatus>> = Mutex::new(None);
static CANCEL: AtomicBool = AtomicBool::new(false);
static INSTALLING: AtomicBool = AtomicBool::new(false);

pub fn status() -> GpuRuntimeStatus {
    let mut guard = STATUS.lock().unwrap_or_else(|err| err.into_inner());
    if let Some(value) = guard.as_ref() {
        return value.clone();
    }
    let detected = read_installed_status().unwrap_or_default();
    *guard = Some(detected.clone());
    detected
}

pub fn cancel_install() -> GpuRuntimeStatus {
    CANCEL.store(true, Ordering::SeqCst);
    if !INSTALLING.load(Ordering::SeqCst) {
        return status();
    }
    update_status(
        GpuRuntimeState::Cancelled,
        0,
        "Cancelling GPU runtime install",
        false,
    )
}

pub fn installed_bin_paths() -> Vec<PathBuf> {
    let root = install_root().join("site-packages").join("nvidia");
    let mut bins = Vec::new();
    let Ok(entries) = fs::read_dir(root) else {
        return bins;
    };
    for entry in entries.flatten() {
        let bin = entry.path().join("bin");
        if bin.is_dir() {
            bins.push(bin);
        }
    }
    bins.sort();
    bins
}

pub fn install<F>(mut progress: F) -> Result<GpuRuntimeStatus, String>
where
    F: FnMut(&GpuRuntimeStatus),
{
    if INSTALLING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("GPU runtime installation is already running".to_string());
    }
    CANCEL.store(false, Ordering::SeqCst);
    let result = install_inner(&mut progress);
    INSTALLING.store(false, Ordering::SeqCst);
    if let Err(reason) = &result {
        let state = if CANCEL.load(Ordering::SeqCst) {
            GpuRuntimeState::Cancelled
        } else {
            GpuRuntimeState::Failed
        };
        let value = update_status(state, 0, reason, false);
        progress(&value);
    }
    result
}

fn install_inner(progress: &mut impl FnMut(&GpuRuntimeStatus)) -> Result<GpuRuntimeStatus, String> {
    let root = runtime_parent();
    fs::create_dir_all(&root).map_err(|err| format!("create GPU runtime directory: {err}"))?;
    let staging = root.join(format!(".{RUNTIME_VERSION}.installing"));
    if staging.exists() {
        fs::remove_dir_all(&staging)
            .map_err(|err| format!("clear stale GPU runtime staging directory: {err}"))?;
    }
    fs::create_dir_all(&staging)
        .map_err(|err| format!("create GPU runtime staging directory: {err}"))?;

    let result = (|| {
        let total: u64 = ARTIFACTS.iter().map(|item| item.size).sum();
        let mut downloaded = 0;
        for artifact in ARTIFACTS {
            let wheel = staging.join(artifact.filename);
            emit(
                progress,
                GpuRuntimeState::Downloading,
                downloaded,
                total,
                &format!("Downloading {}", artifact.filename),
                false,
            );
            download_artifact(artifact, &wheel, &mut downloaded, total, progress)?;
            emit(
                progress,
                GpuRuntimeState::Verifying,
                downloaded,
                total,
                &format!("Verifying {}", artifact.filename),
                false,
            );
            verify_sha256(&wheel, artifact.sha256)?;
        }
        emit(
            progress,
            GpuRuntimeState::Installing,
            total,
            total,
            "Installing GPU libraries",
            false,
        );
        let site_packages = staging.join("site-packages");
        for artifact in ARTIFACTS {
            extract_wheel(&staging.join(artifact.filename), &site_packages)?;
        }
        for artifact in ARTIFACTS {
            let _ = fs::remove_file(staging.join(artifact.filename));
        }
        let manifest = InstalledManifest {
            version: RUNTIME_VERSION.to_string(),
            artifacts: ARTIFACTS
                .iter()
                .map(|item| item.filename.to_string())
                .collect(),
            cuda_verified: false,
        };
        fs::write(
            staging.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).map_err(|err| err.to_string())?,
        )
        .map_err(|err| format!("write GPU runtime manifest: {err}"))?;
        let destination = install_root();
        if destination.exists() {
            fs::remove_dir_all(&destination)
                .map_err(|err| format!("replace old GPU runtime: {err}"))?;
        }
        fs::rename(&staging, &destination).map_err(|err| format!("activate GPU runtime: {err}"))?;
        let ready = update_status(
            GpuRuntimeState::Ready,
            total,
            "GPU runtime installed. Restart VoiceWave to activate it.",
            true,
        );
        progress(&ready);
        Ok(ready)
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

fn download_artifact(
    artifact: &Artifact,
    target: &Path,
    downloaded: &mut u64,
    total: u64,
    progress: &mut impl FnMut(&GpuRuntimeStatus),
) -> Result<(), String> {
    let response = ureq::get(artifact.url)
        .call()
        .map_err(|err| format!("download {}: {err}", artifact.filename))?;
    let mut reader = response.into_reader();
    let mut output =
        File::create(target).map_err(|err| format!("create {}: {err}", artifact.filename))?;
    let mut buffer = vec![0_u8; CHUNK_SIZE];
    loop {
        check_cancelled()?;
        let count = reader
            .read(&mut buffer)
            .map_err(|err| format!("read {}: {err}", artifact.filename))?;
        if count == 0 {
            break;
        }
        output
            .write_all(&buffer[..count])
            .map_err(|err| format!("write {}: {err}", artifact.filename))?;
        *downloaded = downloaded.saturating_add(count as u64);
        emit(
            progress,
            GpuRuntimeState::Downloading,
            *downloaded,
            total,
            &format!("Downloading {}", artifact.filename),
            false,
        );
    }
    let actual = output.metadata().map_err(|err| err.to_string())?.len();
    if actual != artifact.size {
        return Err(format!(
            "{} size mismatch: expected {}, received {}",
            artifact.filename, artifact.size, actual
        ));
    }
    Ok(())
}

fn verify_sha256(path: &Path, expected: &str) -> Result<(), String> {
    check_cancelled()?;
    let mut input =
        File::open(path).map_err(|err| format!("open artifact for verification: {err}"))?;
    let mut digest = Sha256::new();
    io::copy(&mut input, &mut digest).map_err(|err| format!("hash artifact: {err}"))?;
    let actual = format!("{:x}", digest.finalize());
    if actual != expected {
        return Err(format!(
            "download integrity check failed for {}",
            path.file_name().unwrap_or_default().to_string_lossy()
        ));
    }
    Ok(())
}

fn extract_wheel(wheel: &Path, destination: &Path) -> Result<(), String> {
    check_cancelled()?;
    let file = File::open(wheel).map_err(|err| format!("open wheel: {err}"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|err| format!("open wheel archive: {err}"))?;
    for index in 0..archive.len() {
        check_cancelled()?;
        let mut entry = archive
            .by_index(index)
            .map_err(|err| format!("read wheel entry: {err}"))?;
        let Some(relative) = entry.enclosed_name() else {
            return Err("wheel contains an unsafe path".to_string());
        };
        if !relative.starts_with("nvidia") {
            continue;
        }
        let target = destination.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(|err| err.to_string())?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        let mut output =
            File::create(&target).map_err(|err| format!("create extracted GPU library: {err}"))?;
        io::copy(&mut entry, &mut output).map_err(|err| format!("extract GPU library: {err}"))?;
    }
    Ok(())
}

fn emit(
    progress: &mut impl FnMut(&GpuRuntimeStatus),
    state: GpuRuntimeState,
    downloaded: u64,
    total: u64,
    message: &str,
    restart: bool,
) {
    let value = update_status_full(state, downloaded, total, message, restart);
    progress(&value);
}

fn update_status(
    state: GpuRuntimeState,
    downloaded: u64,
    message: &str,
    restart: bool,
) -> GpuRuntimeStatus {
    update_status_full(
        state,
        downloaded,
        ARTIFACTS.iter().map(|item| item.size).sum(),
        message,
        restart,
    )
}

fn update_status_full(
    state: GpuRuntimeState,
    downloaded: u64,
    total: u64,
    message: &str,
    restart: bool,
) -> GpuRuntimeStatus {
    let percent = if total == 0 {
        0
    } else {
        ((downloaded.saturating_mul(100) / total).min(100)) as u8
    };
    let value = GpuRuntimeStatus {
        state,
        installed_version: (state == GpuRuntimeState::Ready).then(|| RUNTIME_VERSION.to_string()),
        downloaded_bytes: downloaded,
        total_bytes: total,
        percent,
        message: message.to_string(),
        restart_required: restart,
        cuda_verified: false,
    };
    *STATUS.lock().unwrap_or_else(|err| err.into_inner()) = Some(value.clone());
    value
}

fn check_cancelled() -> Result<(), String> {
    if CANCEL.load(Ordering::SeqCst) {
        Err("GPU runtime installation cancelled".to_string())
    } else {
        Ok(())
    }
}

fn runtime_parent() -> PathBuf {
    ProjectDirs::from("com", "voicewave", "localcore")
        .map(|dirs| {
            dirs.data_dir()
                .join("runtime-support")
                .join("faster-whisper")
                .join("gpu")
        })
        .unwrap_or_else(|| PathBuf::from("runtime-support/faster-whisper/gpu"))
}

fn install_root() -> PathBuf {
    runtime_parent().join(RUNTIME_VERSION)
}

fn read_installed_status() -> Option<GpuRuntimeStatus> {
    let bytes = fs::read(install_root().join("manifest.json")).ok()?;
    let manifest: InstalledManifest = serde_json::from_slice(&bytes).ok()?;
    if manifest.version != RUNTIME_VERSION || installed_bin_paths().is_empty() {
        return None;
    }
    Some(GpuRuntimeStatus {
        state: GpuRuntimeState::Ready,
        installed_version: Some(manifest.version),
        downloaded_bytes: ARTIFACTS.iter().map(|item| item.size).sum(),
        total_bytes: ARTIFACTS.iter().map(|item| item.size).sum(),
        percent: 100,
        message: "GPU runtime is installed".to_string(),
        restart_required: false,
        cuda_verified: manifest.cuda_verified,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pinned_artifacts_have_sha256_and_sizes() {
        assert!(ARTIFACTS
            .iter()
            .all(|item| item.sha256.len() == 64 && item.size > 0));
    }
    #[test]
    fn progress_is_bounded() {
        assert_eq!(
            update_status_full(GpuRuntimeState::Downloading, 20, 10, "test", false).percent,
            100
        );
    }
}
