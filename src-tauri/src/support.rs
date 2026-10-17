//! What a user can hand us when something breaks: a rolling log file, a panic
//! hook that writes into it, and the helpers behind the in-app "Report a
//! problem" commands.
//!
//! Privacy rule for every log line in the crate: never log transcript text,
//! dictionary terms, snippet contents, clipboard contents or audio. Log sizes,
//! counts, ids and the app's own paths instead.

use std::path::PathBuf;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Daily files, so this is roughly a week of history.
const LOG_FILES_KEPT: usize = 7;
const ISSUE_FORM_URL: &str =
    "https://github.com/rithwik1510/VoiceWave-Open-Source-WhisperFlow-Alternative/issues/new";
const MAX_ENGINE_CHARS: usize = 120;

/// `%LOCALAPPDATA%\voicewave\localcore\data\logs` on Windows: the same
/// `ProjectDirs` root every store uses, under its local (non-roaming) half.
pub fn log_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("com", "voicewave", "localcore")
        .map(|dirs| dirs.data_local_dir().join("logs"))
}

/// Installs the global subscriber and the panic hook, then logs one startup
/// line. Never fails the launch: without a writable log dir the app still
/// runs, it just has nowhere to write.
///
/// Level: `info` by default; `VOICEWAVE_LOG` (then `RUST_LOG`) takes any
/// `EnvFilter` directive, e.g. `VOICEWAVE_LOG=voicewave_core_lib=debug`.
pub fn init() {
    let filter = std::env::var("VOICEWAVE_LOG")
        .or_else(|_| std::env::var("RUST_LOG"))
        .ok()
        .and_then(|directives| EnvFilter::try_new(directives).ok())
        .unwrap_or_else(|| EnvFilter::new("info"));

    let dir = log_dir();
    let mut file_error = None;
    // Deliberately the blocking appender, not `tracing_appender::non_blocking`:
    // each event is written to the file before the macro returns, so a
    // panic's last lines (and the hook's own report) reach disk before the
    // process dies. Volume is a few lines per dictation, so the write cost is
    // irrelevant.
    let file_layer = dir.as_ref().and_then(|dir| {
        RollingFileAppender::builder()
            .rotation(Rotation::DAILY)
            .filename_prefix("voicewave")
            .filename_suffix("log")
            .max_log_files(LOG_FILES_KEPT)
            .build(dir)
            .map_err(|err| file_error = Some(err))
            .ok()
            .map(|appender| fmt::layer().with_writer(appender).with_ansi(false))
    });
    // Dev builds keep a console (release is `windows_subsystem = "windows"`).
    let stderr_layer = cfg!(debug_assertions).then(|| fmt::layer().with_writer(std::io::stderr));

    // `try_init` also bridges the `log` crate (tauri, wry), so their warnings
    // land in the same file.
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(file_layer)
        .with(stderr_layer)
        .try_init();

    install_panic_hook();

    if let Some(err) = file_error {
        tracing::warn!("log file unavailable: {err}");
    }
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        os = %os_version(),
        log_dir = %dir.as_deref().map(|dir| dir.display().to_string()).unwrap_or_default(),
        "VoiceWave starting"
    );
}

fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info.payload();
        let message = payload
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
            .unwrap_or("<non-string panic payload>");
        let location = info
            .location()
            .map(ToString::to_string)
            .unwrap_or_else(|| "<unknown>".to_string());
        let thread = std::thread::current();
        // Release binaries are symbol-stripped, so frames are addresses only;
        // still enough to tell "same crash" apart across reports.
        let backtrace = std::backtrace::Backtrace::force_capture();
        tracing::error!(
            target: "panic",
            thread = thread.name().unwrap_or("<unnamed>"),
            %location,
            "panic: {}\n{backtrace}",
            redact_panic_message(message)
        );
        previous(info);
    }));
}

/// std's string-slicing panics quote the string being sliced (e.g. "byte
/// index 9 is not a char boundary ... of `<transcript>`"), which would put
/// dictated text in the log. Everything from the first backtick is dropped.
fn redact_panic_message(message: &str) -> String {
    match message.find('`') {
        Some(index) => format!("{}<text redacted>", &message[..index]),
        None => message.to_string(),
    }
}

/// Human-readable Windows version, e.g. "Windows 11 Home 24H2 (build
/// 26200.1234)", read from `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion`.
#[cfg(windows)]
pub fn os_version() -> String {
    use windows_sys::Win32::{
        Foundation::ERROR_SUCCESS,
        System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ},
    };

    let key = wide(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");
    let read_string = |name: &str| -> Option<String> {
        let name = wide(name);
        let mut buffer = [0u16; 256];
        let mut size = std::mem::size_of_val(&buffer) as u32;
        // SAFETY: key/name are nul-terminated UTF-16; `size` is the buffer's
        // byte length, and RRF_RT_REG_SZ guarantees a nul-terminated result.
        let status = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buffer.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if status != ERROR_SUCCESS {
            return None;
        }
        let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        let value = String::from_utf16_lossy(&buffer[..len]).trim().to_string();
        (!value.is_empty()).then_some(value)
    };
    let read_dword = |name: &str| -> Option<u32> {
        let name = wide(name);
        let mut value = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        // SAFETY: as above, with a 4-byte out buffer for a REG_DWORD.
        let status = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_DWORD,
                std::ptr::null_mut(),
                (&mut value as *mut u32).cast(),
                &mut size,
            )
        };
        (status == ERROR_SUCCESS).then_some(value)
    };

    let build = read_string("CurrentBuild");
    let mut product = read_string("ProductName").unwrap_or_else(|| "Windows".to_string());
    // Windows 11 never updated ProductName; build 22000 is its first release.
    if build
        .as_deref()
        .and_then(|b| b.parse::<u32>().ok())
        .is_some_and(|b| b >= 22_000)
    {
        product = product.replacen("Windows 10", "Windows 11", 1);
    }
    if let Some(display_version) = read_string("DisplayVersion") {
        product = format!("{product} {display_version}");
    }
    match (build, read_dword("UBR")) {
        (Some(build), Some(ubr)) => format!("{product} (build {build}.{ubr})"),
        (Some(build), None) => format!("{product} (build {build})"),
        (None, _) => product,
    }
}

#[cfg(not(windows))]
pub fn os_version() -> String {
    std::env::consts::OS.to_string()
}

/// The prefilled bug-report form. Only `engine` comes from the frontend, so it
/// is length-capped and stripped of control characters; host and path are
/// fixed here.
pub fn issue_report_url(engine: &str) -> Result<String, String> {
    let engine: String = engine
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_ENGINE_CHARS)
        .collect();
    let os = os_version();
    url::Url::parse_with_params(
        ISSUE_FORM_URL,
        [
            ("template", "bug_report.yml"),
            ("version", env!("CARGO_PKG_VERSION")),
            ("os", os.as_str()),
            ("engine", engine.trim()),
        ],
    )
    .map(String::from)
    .map_err(|err| format!("failed to build the issue URL: {err}"))
}

/// Hands a URL or folder to the shell (default browser / Explorer).
#[cfg(windows)]
pub fn shell_open(target: &std::ffi::OsStr) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};

    let target: Vec<u16> = target.encode_wide().chain(std::iter::once(0)).collect();
    let verb = wide("open");
    // SAFETY: verb/target are nul-terminated UTF-16 that outlive the call;
    // null owner window, parameters and directory are all permitted.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            target.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // The return is a legacy pseudo-HINSTANCE: values above 32 mean success.
    let code = result as isize;
    if code > 32 {
        Ok(())
    } else {
        Err(format!("the shell could not open it (code {code})"))
    }
}

#[cfg(not(windows))]
pub fn shell_open(_target: &std::ffi::OsStr) -> Result<(), String> {
    Err("opening links and folders is only supported on Windows".to_string())
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
