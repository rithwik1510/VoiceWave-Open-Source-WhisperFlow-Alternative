//! What a user can hand us when something breaks: a rolling log file, a panic
//! hook that writes into it (and notes the panic's area in `panics.log` for
//! telemetry, and in the session file), the diagnostics report, and the helpers behind the in-app
//! "Report a problem" commands.
//!
//! Privacy rule for every log line in the crate: never log transcript text,
//! dictionary terms, snippet contents, clipboard contents or audio. Log sizes,
//! counts, ids and the app's own paths instead. The diagnostics report quotes
//! warning and error lines, with the user's profile folder redacted.

use crate::telemetry::PanicArea;
use std::path::PathBuf;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Daily files, so this is roughly a week of history.
const LOG_FILES_KEPT: usize = 7;
const ISSUE_FORM_URL: &str =
    "https://github.com/rithwik1510/VoiceWave-Open-Source-WhisperFlow-Alternative/issues/new";
const MAX_ENGINE_CHARS: usize = 120;
/// Browsers and GitHub reject much longer prefilled-issue URLs.
const MAX_ISSUE_URL_CHARS: usize = 7_500;
/// One panic area per line, read and deleted by `drain_telemetry`.
const PANIC_FILE: &str = "panics.log";
/// A panic loop must not grow the file: past this, panics go unrecorded.
const MAX_PANIC_FILE_BYTES: u64 = 4 * 1024;
const DIAGNOSTICS_WARNING: &str = "Check before posting: this includes your microphone's name and Windows version. Never audio or anything you dictated.";
const MAX_PROBLEM_LINES: usize = 20;
const MAX_PROBLEM_LINE_CHARS: usize = 300;
const MAX_REPORT_BYTES: usize = 6 * 1024;

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
        note_panic_area(info.location().map_or(PanicArea::Other, |location| {
            PanicArea::from_file(location.file())
        }));
        crate::session::mark_panicked();
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

/// Appends the area (never the message or path) for the next
/// `drain_telemetry`. Runs inside the panic hook, so it must not panic:
/// every error is ignored.
fn note_panic_area(area: PanicArea) {
    use std::io::Write;
    let Some(path) = log_dir().map(|dir| dir.join(PANIC_FILE)) else {
        return;
    };
    if std::fs::metadata(&path).is_ok_and(|meta| meta.len() > MAX_PANIC_FILE_BYTES) {
        return;
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(file, "{}", area.as_str());
    }
}

/// Panic areas noted since the last call (this run and earlier ones).
/// Deletes the file.
pub fn take_panic_areas() -> Vec<PanicArea> {
    let Some(path) = log_dir().map(|dir| dir.join(PANIC_FILE)) else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let _ = std::fs::remove_file(&path);
    text.lines()
        .filter_map(|line| PanicArea::parse(line.trim()))
        .collect()
}

/// `C:\Users\<name>` → `%USERPROFILE%`, ignoring ASCII case like Windows.
/// Also catches the Debug-escaped form (`C:\\Users\\<name>`, e.g. a path
/// printed with `{:?}`) and the forward-slash form (`C:/Users/<name>`).
pub fn redact_profile_dir(text: &str) -> String {
    let profile = std::env::var("USERPROFILE")
        .unwrap_or_default()
        .trim_end_matches(['\\', '/'])
        .to_ascii_lowercase();
    if profile.len() <= 3 {
        return text.to_string();
    }
    [
        profile.replace('\\', r"\\"),
        profile.replace('\\', "/"),
        profile,
    ]
    .iter()
    .fold(text.to_string(), |text, needle| {
        replace_ignoring_ascii_case(&text, needle)
    })
}

/// Every match of the lowercase `needle` in `text` → `%USERPROFILE%`.
fn replace_ignoring_ascii_case(text: &str, needle: &str) -> String {
    // ASCII lowercasing keeps byte offsets, so they index `text` too.
    let haystack = text.to_ascii_lowercase();
    let mut redacted = String::with_capacity(text.len());
    let mut at = 0;
    while let Some(found) = haystack[at..].find(needle) {
        redacted.push_str(&text[at..at + found]);
        redacted.push_str("%USERPROFILE%");
        at += found + needle.len();
    }
    redacted.push_str(&text[at..]);
    redacted
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

/// What the diagnostics report says about the app's setup, gathered by the
/// command from settings and the speech engine status. Already worded for
/// people.
pub struct DiagnosticsFacts {
    pub launched_at_login: bool,
    pub cpu_support: &'static str,
    pub active_model: String,
    pub model_installed: bool,
    pub running_on: String,
    pub gpu_name: Option<String>,
    pub gpu_check: &'static str,
    /// `None` = the system default microphone.
    pub microphone: Option<String>,
    pub hotkey: String,
}

/// Plain-text report for bug reports: setup facts plus the newest log's
/// recent warnings and errors. Never audio or dictated text.
pub struct DiagnosticsReport {
    summary: String,
    /// Oldest first, redacted and capped.
    problems: Vec<String>,
}

impl DiagnosticsReport {
    pub fn new(facts: &DiagnosticsFacts) -> Self {
        let yes_no = |value: bool| if value { "yes" } else { "no" };
        let summary = format!(
            "{DIAGNOSTICS_WARNING}\n\n\
             VoiceWave {version}\n\
             Windows: {os}\n\
             CPU support: {cpu}\n\
             Launched at login: {login}\n\n\
             Speech model: {model} (installed: {installed})\n\
             Running on: {running_on}\n\
             GPU: {gpu}\n\
             GPU check: {gpu_check}\n\n\
             Microphone: {mic}\n\
             Hotkey: {hotkey}\n",
            version = env!("CARGO_PKG_VERSION"),
            os = os_version(),
            cpu = facts.cpu_support,
            login = yes_no(facts.launched_at_login),
            model = facts.active_model,
            installed = yes_no(facts.model_installed),
            running_on = facts.running_on,
            gpu = facts.gpu_name.as_deref().unwrap_or("none"),
            gpu_check = facts.gpu_check,
            mic = facts.microphone.as_deref().unwrap_or("system default"),
            hotkey = facts.hotkey,
        );
        Self {
            summary,
            problems: log_dir()
                .map(|dir| recent_problems(&dir))
                .unwrap_or_default(),
        }
    }

    /// The report with only the newest `keep` problem lines.
    fn render(&self, keep: usize) -> String {
        let lines = &self.problems[self.problems.len().saturating_sub(keep)..];
        let mut text = self.summary.clone();
        if lines.is_empty() {
            text.push_str("\nRecent problems: none in the newest log file\n");
        } else {
            text.push_str("\nRecent problems (newest log file):\n");
            for line in lines {
                text.push_str(line);
                text.push('\n');
            }
        }
        text
    }

    /// The full report, dropping the oldest log lines to stay near 6 KB.
    pub fn text(&self) -> String {
        (0..=self.problems.len())
            .rev()
            .map(|keep| self.render(keep))
            .find(|text| text.len() <= MAX_REPORT_BYTES)
            .unwrap_or_else(|| self.render(0))
    }
}

/// The first line of each of the last `MAX_PROBLEM_LINES` WARN/ERROR events
/// in the newest daily log. An event starts with a timestamp line; the lines
/// after it (a panic's backtrace) are skipped.
fn recent_problems(dir: &std::path::Path) -> Vec<String> {
    let newest = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("voicewave.") && name.ends_with(".log"))
        // Daily files are named voicewave.YYYY-MM-DD.log.
        .max();
    let Some(text) = newest
        .and_then(|name| std::fs::read(dir.join(name)).ok())
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
    else {
        return Vec::new();
    };
    let mut problems: Vec<&str> = text
        .lines()
        .filter(|line| {
            let mut fields = line.split_whitespace();
            let starts_event = fields.next().is_some_and(|stamp| {
                stamp.len() >= 20
                    && stamp.as_bytes()[..4].iter().all(u8::is_ascii_digit)
                    && stamp.as_bytes()[4] == b'-'
                    && stamp.as_bytes()[10] == b'T'
            });
            starts_event && matches!(fields.next(), Some("WARN" | "ERROR"))
        })
        .collect();
    let skip = problems.len().saturating_sub(MAX_PROBLEM_LINES);
    problems
        .drain(skip..)
        .map(|line| {
            let line = redact_profile_dir(line.trim_end());
            if line.chars().count() <= MAX_PROBLEM_LINE_CHARS {
                return line;
            }
            let mut cut: String = line.chars().take(MAX_PROBLEM_LINE_CHARS - 1).collect();
            cut.push('…');
            cut
        })
        .collect()
}

/// The prefilled bug-report form. Only `engine` comes from the frontend, so it
/// is length-capped and stripped of control characters; host and path are
/// fixed here. The report goes in `diagnostics`; to keep the URL under
/// `MAX_ISSUE_URL_CHARS` its oldest log lines go first, then its tail.
pub fn issue_report_url(engine: &str, report: &DiagnosticsReport) -> Result<String, String> {
    let engine: String = engine
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_ENGINE_CHARS)
        .collect();
    let os = os_version();
    let build = |diagnostics: &str| {
        url::Url::parse_with_params(
            ISSUE_FORM_URL,
            [
                ("template", "bug_report.yml"),
                ("version", env!("CARGO_PKG_VERSION")),
                ("os", os.as_str()),
                ("engine", engine.trim()),
                ("diagnostics", diagnostics),
            ],
        )
        .map(String::from)
        .map_err(|err| format!("failed to build the issue URL: {err}"))
    };
    for keep in (0..=report.problems.len()).rev() {
        let url = build(&report.render(keep))?;
        if url.len() <= MAX_ISSUE_URL_CHARS {
            return Ok(url);
        }
    }
    // Still too long with no log lines (a very long microphone name): keep
    // the longest prefix that fits. Characters encode to 1-12 bytes, so
    // search rather than estimate.
    let chars: Vec<char> = report.render(0).chars().collect();
    let prefix = |len: usize| chars[..len].iter().collect::<String>();
    // Invariant: `fits` chars fit, `too_long` chars do not.
    let (mut fits, mut too_long) = (0, chars.len());
    while fits + 1 < too_long {
        let mid = (fits + too_long) / 2;
        if build(&prefix(mid))?.len() <= MAX_ISSUE_URL_CHARS {
            fits = mid;
        } else {
            too_long = mid;
        }
    }
    build(&prefix(fits))
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
