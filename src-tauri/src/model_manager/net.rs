//! HTTP side of model downloads: the shared agent, and turning transport and
//! disk errors into a `DownloadFailureStage` (what the user is told, what
//! telemetry reports, and whether `DownloadPlan::run` retries).

use super::ModelError;
use serde::{Deserialize, Serialize};
use std::{
    error::Error as _,
    fmt, io,
    sync::{Arc, OnceLock},
    time::Duration,
};

/// Where a model download failed. Sent to the frontend and, as the enum name
/// only, to usage telemetry.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DownloadFailureStage {
    Dns,
    Connect,
    Tls,
    Http4xx,
    Http5xx,
    Timeout,
    StreamEnded,
    Checksum,
    DiskFull,
    Write,
    Rename,
    Other,
}

impl fmt::Display for DownloadFailureStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Dns => "dns",
            Self::Connect => "connect",
            Self::Tls => "tls",
            Self::Http4xx => "http4xx",
            Self::Http5xx => "http5xx",
            Self::Timeout => "timeout",
            Self::StreamEnded => "streamEnded",
            Self::Checksum => "checksum",
            Self::DiskFull => "diskFull",
            Self::Write => "write",
            Self::Rename => "rename",
            Self::Other => "other",
        })
    }
}

impl DownloadFailureStage {
    /// The failed-status message: plain and actionable. Raw error detail goes
    /// to the log, never here.
    pub fn user_message(self, http_status: Option<u16>) -> String {
        let code = http_status
            .map(|code| format!(" (HTTP {code})"))
            .unwrap_or_default();
        match self {
            Self::Dns | Self::Connect => {
                "Couldn't reach Hugging Face. Check your internet connection, then try again."
                    .to_string()
            }
            // Also what an expired or not-yet-valid certificate looks like, so
            // a wrong PC clock is named too.
            Self::Tls => "The secure connection to Hugging Face was refused. Check that your \
                PC's date and time are right, and allow VoiceWave in your antivirus or \
                firewall, then try again."
                .to_string(),
            Self::Http4xx => format!(
                "Hugging Face refused the download{code}. Try again later; if it keeps \
                 happening, please report it."
            ),
            Self::Http5xx => {
                format!("Hugging Face is having trouble{code}. Try again in a few minutes.")
            }
            // Shown only after the automatic retries ran out.
            Self::Timeout => "The connection to Hugging Face kept timing out. Check your \
                internet connection, then press Retry to continue where the download stopped."
                .to_string(),
            Self::StreamEnded => "The connection kept dropping. Press Retry to continue where \
                the download stopped."
                .to_string(),
            Self::Checksum => "The downloaded file was damaged, so it was discarded. Press Retry \
                to download it again."
                .to_string(),
            Self::DiskFull => {
                "Not enough disk space for this model. Free up some space, then press Retry."
                    .to_string()
            }
            Self::Write => "Couldn't save the model file. Another program (often antivirus) may \
                be holding it; press Retry in a moment."
                .to_string(),
            Self::Rename => "The model downloaded, but another program (often antivirus) is \
                holding the file. Press Retry in a moment; nothing downloads again."
                .to_string(),
            Self::Other => "The download failed. Press Retry to try again.".to_string(),
        }
    }
}

/// Windows "disk full" (ERROR_DISK_FULL / ERROR_HANDLE_DISK_FULL).
pub(super) fn is_disk_full(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::StorageFull || matches!(err.raw_os_error(), Some(112 | 39))
}

/// A stalled socket: Windows reports a read timeout as `TimedOut`, other
/// platforms as `WouldBlock`.
fn is_timeout(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
    )
}

/// The shared download agent, built on first use: rustls trusting both the
/// bundled Mozilla roots and the Windows certificate store (antivirus HTTPS
/// scanning and corporate TLS inspection install their own root there).
/// Proxy variables from the environment are deliberately ignored: ureq skips
/// `NO_PROXY` and has no SOCKS support here, so a dev's `ALL_PROXY` would turn
/// a working direct download into a confusing "couldn't reach" failure.
pub(super) fn http_agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        let builder = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(15))
            // A stall longer than this (sleep, Wi-Fi drop) ends the attempt;
            // `DownloadPlan::run` then resumes from the bytes on disk.
            .timeout_read(Duration::from_secs(30))
            .user_agent(concat!("voicewave/", env!("CARGO_PKG_VERSION")));
        match tls_config() {
            Some(config) => builder.tls_config(config),
            None => builder,
        }
        .build()
    })
}

fn tls_config() -> Option<Arc<rustls::ClientConfig>> {
    let mut roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let bundled = roots.len();
    // The loader unwraps a couple of Win32 results; a panic there must not
    // take every model download down with it.
    match std::panic::catch_unwind(rustls_native_certs::load_native_certs) {
        Ok(native) => {
            let (added, ignored) = roots.add_parsable_certificates(native.certs);
            for err in &native.errors {
                tracing::warn!("couldn't read Windows certificate store: {err}");
            }
            tracing::info!(
                "download client trusts {bundled} bundled + {added} Windows roots ({ignored} unparsable)"
            );
        }
        Err(_) => tracing::warn!("Windows certificate store load panicked; using bundled roots"),
    }
    // `builder_with_provider`, not `builder()`: no reliance on a process-wide
    // crypto provider (same choice ureq makes for its own default).
    let config = rustls::ClientConfig::builder_with_provider(
        rustls::crypto::ring::default_provider().into(),
    )
    .with_safe_default_protocol_versions()
    .map_err(|err| tracing::warn!("custom TLS config unavailable, using ureq defaults: {err}"))
    .ok()?
    .with_root_certificates(roots)
    .with_no_client_auth();
    Some(Arc::new(config))
}

/// Scheme, host and path only: signed CDN redirects carry credentials in the
/// query string, which must never reach the log.
fn redact_url(raw: &str) -> String {
    match url::Url::parse(raw) {
        Ok(url) => format!(
            "{}://{}{}",
            url.scheme(),
            url.host_str().unwrap_or(""),
            url.path()
        ),
        Err(_) => "<url>".to_string(),
    }
}

/// The certificate or protocol error rustls raised during the handshake, if
/// that is what ended the connection. ureq wraps it as an `io::Error` source.
fn rustls_error(err: &ureq::Transport) -> Option<&rustls::Error> {
    let mut source = err.source();
    while let Some(current) = source {
        if let Some(found) = current.downcast_ref::<rustls::Error>() {
            return Some(found);
        }
        if let Some(found) = current
            .downcast_ref::<io::Error>()
            .and_then(|io_err| io_err.get_ref())
            .and_then(|inner| inner.downcast_ref::<rustls::Error>())
        {
            return Some(found);
        }
        source = current.source();
    }
    None
}

fn transport_io_error(err: &ureq::Transport) -> Option<&io::Error> {
    err.source()
        .and_then(|source| source.downcast_ref::<io::Error>())
}

fn network(
    stage: DownloadFailureStage,
    http_status: Option<u16>,
    retryable: bool,
    detail: String,
) -> ModelError {
    ModelError::Network {
        stage,
        http_status,
        retryable,
        detail,
    }
}

/// Classifies a failed request (before any body byte arrived).
///
/// Retried: DNS, connect, timeouts, a connection reset mid-handshake, and
/// HTTP 408/429/5xx. Not retried: a TLS rejection (certificate or protocol;
/// the same machine fails the same way again), other 4xx, and request/proxy
/// configuration errors.
pub(super) fn classify_request_error(err: ureq::Error) -> ModelError {
    use DownloadFailureStage as Stage;
    match err {
        ureq::Error::Status(code, response) => {
            let detail = format!("HTTP {code} from {}", redact_url(response.get_url()));
            if code >= 500 {
                network(Stage::Http5xx, Some(code), true, detail)
            } else {
                network(
                    Stage::Http4xx,
                    Some(code),
                    matches!(code, 408 | 429),
                    detail,
                )
            }
        }
        ureq::Error::Transport(transport) => {
            // Built from parts: the transport's Display embeds the full URL.
            let mut detail = transport.kind().to_string();
            if let Some(message) = transport.message() {
                detail = format!("{detail}: {message}");
            }
            if let Some(source) = transport.source() {
                detail = format!("{detail}: {source}");
            }
            if let Some(url) = transport.url() {
                detail = format!("{detail} ({})", redact_url(url.as_str()));
            }
            let timed_out = transport_io_error(&transport).is_some_and(is_timeout);
            match transport.kind() {
                ureq::ErrorKind::Dns => network(Stage::Dns, None, true, detail),
                _ if rustls_error(&transport).is_some() => network(Stage::Tls, None, false, detail),
                ureq::ErrorKind::ConnectionFailed
                | ureq::ErrorKind::ProxyConnect
                | ureq::ErrorKind::Io => {
                    let stage = if timed_out {
                        Stage::Timeout
                    } else {
                        Stage::Connect
                    };
                    network(stage, None, true, detail)
                }
                // A garbled status line or header is usually a cut-off
                // response; one more try is cheap.
                ureq::ErrorKind::BadStatus | ureq::ErrorKind::BadHeader => {
                    network(Stage::Other, None, true, detail)
                }
                _ => network(Stage::Other, None, false, detail),
            }
        }
    }
}

/// Classifies a failed read of the response body. Every such error is a
/// network problem (stall, reset, early close), so all are retried.
pub(super) fn classify_body_error(err: io::Error) -> ModelError {
    let stage = if is_timeout(&err) {
        DownloadFailureStage::Timeout
    } else {
        DownloadFailureStage::StreamEnded
    };
    network(stage, None, true, err.to_string())
}

impl ModelError {
    /// The stage reported to the user and to telemetry.
    pub fn failure_stage(&self) -> DownloadFailureStage {
        use DownloadFailureStage as Stage;
        match self {
            ModelError::Network { stage, .. } => *stage,
            ModelError::CorruptSource { .. } => Stage::StreamEnded,
            ModelError::ChecksumMismatch { .. } => Stage::Checksum,
            ModelError::LowDisk { .. } => Stage::DiskFull,
            ModelError::Write(err) if is_disk_full(err) => Stage::DiskFull,
            ModelError::Write(_) => Stage::Write,
            ModelError::Rename(_) => Stage::Rename,
            _ => Stage::Other,
        }
    }

    /// Whether `DownloadPlan::run` should resume after a short wait.
    pub(super) fn is_transient(&self) -> bool {
        match self {
            ModelError::Network { retryable, .. } => *retryable,
            ModelError::CorruptSource { .. } => true,
            _ => false,
        }
    }

    /// The failed-status message for this error (see `user_message`).
    pub fn user_message(&self) -> String {
        let http_status = match self {
            ModelError::Network { http_status, .. } => *http_status,
            _ => None,
        };
        self.failure_stage().user_message(http_status)
    }
}
