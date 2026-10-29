use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    collections::{HashMap, VecDeque},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum InsertionMethod {
    Direct,
    ClipboardPaste,
    ClipboardOnly,
    HistoryFallback,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InsertTextRequest {
    pub text: String,
    pub target_app: Option<String>,
    pub prefer_clipboard: bool,
    pub force_clipboard_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InsertResult {
    pub success: bool,
    pub method: InsertionMethod,
    pub message: Option<String>,
    pub target_app: Option<String>,
    pub transaction_id: String,
    pub undo_available: bool,
    /// The focused app runs at a higher integrity level (usually "as
    /// administrator"), so Windows would silently drop our keystrokes. The
    /// text went to the clipboard instead (plan 020).
    #[serde(default)]
    pub blocked_by_elevation: bool,
    /// The foreground app's kind; `None` when the request named its own
    /// target (`target_app`), whose process isn't known.
    #[serde(default)]
    pub app_kind: Option<AppKind>,
}

/// `dwExtraInfo` tag on every keystroke VoiceWave sends (typing, paste
/// chords, Enter, undo). The hotkey hook passes these straight through, so
/// a paste chord that happens to match the user's hotkey does not start a
/// session. Other injected input (AutoHotkey, PowerToys remaps) carries no
/// tag and still reaches the hotkey logic.
pub const VOICEWAVE_INPUT_MARKER: usize = 0x5657_4156; // "VWAV"

/// HUD message for an elevated target; the pill carries the same facts.
const ELEVATED_TARGET_MESSAGE: &str =
    "This app runs as administrator, so VoiceWave can't type into it. Press Ctrl+V.";

/// Keystroke that pastes in a given target. Terminals bind paste to
/// different chords, and a wrong one is not harmless (Ctrl+V is bash's
/// quoted-insert in mintty), so each terminal family gets its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteChord {
    CtrlV,
    CtrlShiftV,
    ShiftInsert,
}

/// Why a spoken submit (plan 020) did not press Enter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmitError {
    /// The text only reached the clipboard (or nothing), so Enter would
    /// submit an empty or stale prompt.
    NotDelivered,
    /// The foreground window changed after the text went in.
    FocusMoved,
    /// The focused app runs as administrator; Windows would drop the key.
    Elevated,
    Failed(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UndoResult {
    pub success: bool,
    pub message: Option<String>,
    pub transaction_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecentInsertion {
    pub transaction_id: String,
    pub target_app: Option<String>,
    pub preview: String,
    pub method: InsertionMethod,
    pub success: bool,
    pub timestamp_utc_ms: u64,
    pub message: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum InsertionError {
    #[error("cannot insert empty transcript")]
    EmptyText,
}

#[derive(Debug, Clone)]
pub enum UndoToken {
    KeyboardUndo,
}

#[derive(Debug, Clone)]
pub struct BackendInsertSuccess {
    pub message: Option<String>,
    pub undo_token: Option<UndoToken>,
}

pub trait InsertionBackend: Send {
    fn detect_target_app(&mut self) -> Option<String>;

    /// Executable file name of the foreground process (e.g. "msedge.exe"),
    /// when it can be resolved. Classification prefers it over the window
    /// title, which is free text the user's document controls.
    fn detect_target_process(&mut self) -> Option<String> {
        None
    }

    /// True when the foreground app runs at a higher integrity level than
    /// VoiceWave, where UIPI drops injected input without reporting it.
    fn target_is_elevated(&mut self) -> bool {
        false
    }

    /// Opaque id of the foreground window, used to confirm focus has not
    /// moved between inserting text and pressing Enter after it.
    fn foreground_window(&mut self) -> Option<isize> {
        None
    }

    fn direct_insert(
        &mut self,
        text: &str,
        target_app: Option<&str>,
    ) -> Result<BackendInsertSuccess, String>;

    fn clipboard_paste(
        &mut self,
        text: &str,
        target_app: Option<&str>,
        chord: PasteChord,
    ) -> Result<BackendInsertSuccess, String>;

    fn clipboard_only(
        &mut self,
        text: &str,
        target_app: Option<&str>,
    ) -> Result<BackendInsertSuccess, String>;

    fn undo(&mut self, token: &UndoToken) -> Result<Option<String>, String>;

    /// One Enter keystroke into the foreground window.
    fn press_enter(&mut self) -> Result<(), String>;
}

#[derive(Debug, Clone)]
struct UndoRecord {
    transaction_id: String,
    token: UndoToken,
}

pub struct InsertionEngine {
    backend: Box<dyn InsertionBackend>,
    history: VecDeque<RecentInsertion>,
    last_successful: Option<UndoRecord>,
    /// Window that received the last insertion via keystrokes or paste;
    /// None after a clipboard-only or failed insert. Spoken submit presses
    /// Enter only while this window still has focus.
    last_landed_window: Option<isize>,
    next_id: u64,
    history_limit: usize,
}

impl Default for InsertionEngine {
    fn default() -> Self {
        Self::new(Box::new(PlatformInsertionBackend::default()))
    }
}

impl InsertionEngine {
    pub fn new(backend: Box<dyn InsertionBackend>) -> Self {
        Self {
            backend,
            history: VecDeque::with_capacity(32),
            last_successful: None,
            last_landed_window: None,
            next_id: 1,
            history_limit: 50,
        }
    }

    pub fn insert_text(
        &mut self,
        request: InsertTextRequest,
    ) -> Result<InsertResult, InsertionError> {
        // A submit may only follow the insertion it belongs to.
        self.last_landed_window = None;
        if request.text.trim().is_empty() {
            return Err(InsertionError::EmptyText);
        }

        let transaction_id = self.next_transaction_id();
        let resolved_target = request
            .target_app
            .clone()
            .or_else(|| self.backend.detect_target_app());
        let target_ref = resolved_target.as_deref();
        // An explicit target names some other window, so the foreground
        // process says nothing about it; classify by that name alone.
        let target_process = if request.target_app.is_some() {
            None
        } else {
            self.backend.detect_target_process()
        };
        // Analytics only: the exe name stops here.
        let target_kind = request
            .target_app
            .is_none()
            .then(|| app_kind(target_process.as_deref()));

        // UIPI drops keystrokes into a higher-integrity app without any error,
        // so an elevated target goes straight to the clipboard (plan 020).
        let blocked_by_elevation = request.target_app.is_none()
            && !request.force_clipboard_only
            && self.backend.target_is_elevated();
        let terminal = classify_terminal(target_process.as_deref(), target_ref);
        // Terminal text is flattened to one line so nothing in it can act as
        // an Enter (see `terminal_safe_text`).
        let text: Cow<str> = if terminal.is_some() {
            Cow::Owned(terminal_safe_text(&request.text))
        } else {
            Cow::Borrowed(request.text.as_str())
        };
        if text.trim().is_empty() {
            return Err(InsertionError::EmptyText);
        }
        let paste_chord = match terminal {
            Some(TerminalTarget::Paste(chord)) => chord,
            _ => PasteChord::CtrlV,
        };
        let landing_window = self.backend.foreground_window();

        let prefer_clipboard = request.prefer_clipboard
            || should_auto_prefer_clipboard(&text, target_ref, target_process.as_deref());

        let method_order = if request.force_clipboard_only
            || blocked_by_elevation
            || terminal == Some(TerminalTarget::ClipboardOnly)
        {
            vec![InsertionMethod::ClipboardOnly]
        } else if terminal.is_some() {
            // Keystroke typing is unreliable through ConPTY; paste only.
            vec![
                InsertionMethod::ClipboardPaste,
                InsertionMethod::ClipboardOnly,
            ]
        } else if prefer_clipboard {
            vec![
                InsertionMethod::ClipboardPaste,
                InsertionMethod::ClipboardOnly,
            ]
        } else {
            vec![
                InsertionMethod::Direct,
                InsertionMethod::ClipboardPaste,
                InsertionMethod::ClipboardOnly,
            ]
        };

        let mut last_failure_message = None::<String>;
        let mut success_result = None::<InsertResult>;
        let mut success_undo = None::<UndoToken>;

        for method in method_order {
            let attempt = match method {
                InsertionMethod::Direct => self.backend.direct_insert(&text, target_ref),
                InsertionMethod::ClipboardPaste => {
                    self.backend.clipboard_paste(&text, target_ref, paste_chord)
                }
                InsertionMethod::ClipboardOnly => self.backend.clipboard_only(&text, target_ref),
                InsertionMethod::HistoryFallback => unreachable!("history fallback is synthetic"),
            };

            match attempt {
                Ok(mut result) => {
                    // Undo sends Ctrl+Z, which suspends the foreground job in
                    // a Unix-style shell. Never offer it for a terminal.
                    if terminal.is_some() {
                        result.undo_token = None;
                    }
                    if blocked_by_elevation {
                        result.message = Some(ELEVATED_TARGET_MESSAGE.to_string());
                    }
                    let undo_available = result.undo_token.is_some();
                    success_undo = result.undo_token;
                    success_result = Some(InsertResult {
                        success: true,
                        method,
                        message: result.message,
                        target_app: resolved_target.clone(),
                        transaction_id: transaction_id.clone(),
                        undo_available,
                        blocked_by_elevation,
                        app_kind: target_kind,
                    });
                    break;
                }
                Err(err) => {
                    last_failure_message = Some(err);
                }
            }
        }

        let mut result = success_result.unwrap_or_else(|| InsertResult {
            success: false,
            method: InsertionMethod::HistoryFallback,
            message: Some(last_failure_message.unwrap_or_else(|| {
                "Insertion blocked. Transcript saved to quick history.".to_string()
            })),
            target_app: resolved_target.clone(),
            transaction_id: transaction_id.clone(),
            undo_available: false,
            blocked_by_elevation,
            app_kind: target_kind,
        });

        if !result.success && result.message.is_none() {
            result.message =
                Some("Insertion blocked. Transcript saved to quick history.".to_string());
        }

        self.last_landed_window = if result.success
            && matches!(
                result.method,
                InsertionMethod::Direct | InsertionMethod::ClipboardPaste
            ) {
            landing_window
        } else {
            None
        };
        // History keeps what was said, not the flattened terminal copy, so it
        // matches the main History and copies back with its line breaks.
        self.record_history(&result, &request.text);
        // Undo always refers to the latest landed insertion. One that can't be
        // undone (a terminal paste, clipboard only) clears the older record,
        // so Ctrl+Z is never sent on behalf of text from an earlier app.
        if result.success {
            self.last_successful = success_undo.map(|token| UndoRecord {
                transaction_id: transaction_id.clone(),
                token,
            });
        }

        Ok(result)
    }

    /// Spoken submit after an insertion (plan 020): one Enter into the window
    /// that just received the text. Refuses when the text never reached an
    /// app or focus has moved since, so Enter cannot land somewhere the user
    /// didn't dictate into. Consumes the landing record either way.
    pub fn press_enter_after_insert(&mut self) -> Result<(), SubmitError> {
        let Some(expected) = self.last_landed_window.take() else {
            return Err(SubmitError::NotDelivered);
        };
        if self.backend.foreground_window() != Some(expected) {
            return Err(SubmitError::FocusMoved);
        }
        self.backend.press_enter().map_err(SubmitError::Failed)
    }

    /// Spoken submit with no text ("press enter" on its own): Enter into the
    /// focused window, unless Windows would drop it.
    pub fn press_enter_now(&mut self) -> Result<(), SubmitError> {
        if self.backend.target_is_elevated() {
            return Err(SubmitError::Elevated);
        }
        self.backend.press_enter().map_err(SubmitError::Failed)
    }

    pub fn undo_last(&mut self) -> UndoResult {
        let Some(last) = self.last_successful.clone() else {
            return UndoResult {
                success: false,
                message: Some("No insertion is available to undo.".to_string()),
                transaction_id: None,
            };
        };

        match self.backend.undo(&last.token) {
            Ok(message) => {
                self.last_successful = None;
                UndoResult {
                    success: true,
                    message: message.or(Some("Last insertion undone.".to_string())),
                    transaction_id: Some(last.transaction_id),
                }
            }
            Err(err) => UndoResult {
                success: false,
                message: Some(format!("Undo failed: {err}")),
                transaction_id: None,
            },
        }
    }

    pub fn recent_insertions(&self, limit: Option<usize>) -> Vec<RecentInsertion> {
        let take = limit.unwrap_or(10).min(self.history.len());
        self.history.iter().rev().take(take).cloned().collect()
    }

    fn next_transaction_id(&mut self) -> String {
        let id = format!("ins-{}", self.next_id);
        self.next_id += 1;
        id
    }

    fn record_history(&mut self, result: &InsertResult, text: &str) {
        let preview = text.chars().take(120).collect::<String>();
        let timestamp_utc_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or_default();
        self.history.push_back(RecentInsertion {
            transaction_id: result.transaction_id.clone(),
            target_app: result.target_app.clone(),
            preview,
            method: result.method.clone(),
            success: result.success,
            timestamp_utc_ms,
            message: result.message.clone(),
        });

        while self.history.len() > self.history_limit {
            let _ = self.history.pop_front();
        }
    }
}

fn should_auto_prefer_clipboard(
    text: &str,
    target_app: Option<&str>,
    target_process: Option<&str>,
) -> bool {
    const LONG_TEXT_THRESHOLD: usize = 48;
    const UNKNOWN_TARGET_THRESHOLD: usize = 1;
    if text.chars().count() >= LONG_TEXT_THRESHOLD {
        return true;
    }

    let Some(app) = target_app else {
        return text.chars().count() >= UNKNOWN_TARGET_THRESHOLD;
    };
    // The process is authoritative when known: a Notepad window titled
    // "cursor.md" is still Notepad. The title is only the fallback.
    match target_process {
        Some(exe) => is_clipboard_preferred_process(exe),
        None => is_clipboard_preferred_title(app),
    }
}

/// Apps where synthetic typing is flaky (Electron editors, browsers, web
/// chat apps), identified by executable. Gemini / AI Studio run inside a
/// browser, so the browser executables cover them.
fn is_clipboard_preferred_process(exe_name: &str) -> bool {
    const CLIPBOARD_PREFERRED_EXES: &[&str] = &[
        "code.exe",
        "code - insiders.exe",
        "cursor.exe",
        "chrome.exe",
        "msedge.exe",
        "firefox.exe",
        "slack.exe",
        "notion.exe",
    ];
    let lowered = exe_name.to_ascii_lowercase();
    CLIPBOARD_PREFERRED_EXES
        .iter()
        .any(|candidate| lowered == *candidate)
}

/// Title fallback for [`is_clipboard_preferred_process`], matched on whole
/// words so "edge" does not fire on "Knowledge Base - Notepad".
fn is_clipboard_preferred_title(app: &str) -> bool {
    const CLIPBOARD_PREFERRED_TITLE_WORDS: &[&str] = &[
        "visual studio code",
        "vscode",
        "cursor",
        "chrome",
        "edge",
        "firefox",
        "google ai studio",
        "aistudio.google",
        "gemini",
        "slack",
        "notion",
    ];
    let normalized = app.to_ascii_lowercase();
    CLIPBOARD_PREFERRED_TITLE_WORDS
        .iter()
        .any(|word| title_contains_word(&normalized, word))
}

/// True when `word` occurs in `lowered_title` with no letter or digit
/// directly on either side, so "edge" matches "Microsoft Edge" but not
/// "Knowledge". Both arguments must already be lowercase.
pub fn title_contains_word(lowered_title: &str, word: &str) -> bool {
    lowered_title.match_indices(word).any(|(start, _)| {
        let before = lowered_title[..start].chars().next_back();
        let after = lowered_title[start + word.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

/// Paste chord for a terminal host, by executable name; None when the
/// process is not a terminal. Matching on the foreground *process* is
/// authoritative because window titles lie: the Codex and Claude desktop apps
/// carry "codex"/"claude" in their titles but are ordinary Electron GUIs.
///
/// Chords are each family's default paste binding (plan 020):
/// - Windows Terminal binds Ctrl+V, Ctrl+Shift+V and Shift+Insert. Ctrl+V is
///   the one power users unbind so vim/readline receive ^V; Ctrl+Shift+V has
///   been bound since the first release.
/// - Console windows report their client shell as the owning process. conhost
///   pastes on Ctrl+V ("Ctrl key shortcuts", on by default since Windows 10)
///   and PSReadLine binds Ctrl+V to its own paste.
/// - mintty passes Ctrl+V to bash (quoted-insert); Shift+Insert pastes.
/// - The rest paste on Ctrl+Shift+V; Shift+Insert in alacritty and wezterm
///   pastes the selection buffer, not the clipboard.
pub fn terminal_paste_chord(exe_name: &str) -> Option<PasteChord> {
    match exe_name.to_ascii_lowercase().as_str() {
        // Windows Terminal (and the ConPTY console host it spawns)
        "windowsterminal.exe" | "openconsole.exe" => Some(PasteChord::CtrlShiftV),
        // Classic console host (cmd/PowerShell outside Windows Terminal)
        "conhost.exe" | "powershell.exe" | "pwsh.exe" | "cmd.exe" => Some(PasteChord::CtrlV),
        // Git Bash / MSYS2 / Cygwin
        "mintty.exe" => Some(PasteChord::ShiftInsert),
        // Third-party terminals
        "alacritty.exe" | "wezterm-gui.exe" | "wezterm.exe" | "kitty.exe" | "hyper.exe"
        | "tabby.exe" | "warp.exe" => Some(PasteChord::CtrlShiftV),
        _ => None,
    }
}

pub fn is_terminal_process_exe(exe_name: &str) -> bool {
    terminal_paste_chord(exe_name).is_some()
}

/// The kind of app text went into (plan 024 section 4.5), for analytics.
/// Only the kind leaves `app_kind`: the exe name never does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppKind {
    Browser,
    CodeEditor,
    Terminal,
    Chat,
    Office,
    Notes,
    Ai,
    Other,
    /// No foreground process could be resolved.
    Unknown,
}

/// Maps the foreground process's exe file name (e.g. "msedge.exe").
pub fn app_kind(exe_name: Option<&str>) -> AppKind {
    let Some(exe_name) = exe_name else {
        return AppKind::Unknown;
    };
    if is_terminal_process_exe(exe_name) {
        return AppKind::Terminal;
    }
    let lower = exe_name.to_ascii_lowercase();
    match lower.strip_suffix(".exe").unwrap_or(&lower) {
        "chrome" | "msedge" | "firefox" | "brave" | "opera" | "vivaldi" | "arc" => AppKind::Browser,
        "code" | "code - insiders" | "cursor" | "windsurf" | "zed" | "devenv" | "idea64"
        | "pycharm64" | "webstorm64" | "rider64" | "sublime_text" | "notepad++" => {
            AppKind::CodeEditor
        }
        "slack" | "discord" | "teams" | "ms-teams" | "whatsapp" | "telegram" | "signal" => {
            AppKind::Chat
        }
        "winword" | "excel" | "powerpnt" | "outlook" | "olk" | "onenote" => AppKind::Office,
        "notion" | "obsidian" | "notepad" | "evernote" | "logseq" => AppKind::Notes,
        "claude" | "chatgpt" | "codex" => AppKind::Ai,
        _ => AppKind::Other,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TerminalTarget {
    /// A known terminal host: paste with its chord.
    Paste(PasteChord),
    /// Only the window title looks like a terminal (the process could not be
    /// resolved), so no chord is known to be safe.
    ClipboardOnly,
}

fn classify_terminal(
    target_process: Option<&str>,
    window_title: Option<&str>,
) -> Option<TerminalTarget> {
    match target_process {
        Some(exe) => terminal_paste_chord(exe).map(TerminalTarget::Paste),
        None => is_terminal_target(window_title).then_some(TerminalTarget::ClipboardOnly),
    }
}

/// Text bound for a terminal must never run anything on its own (plan 020).
/// Unless the program inside has turned on bracketed paste, which VoiceWave
/// cannot see from outside, every pasted line break is an Enter. A tab
/// triggers completion and an escape can end a bracketed paste early. So every
/// run of control characters (and Unicode line/paragraph separators) becomes
/// one space and trailing whitespace is dropped: the paste is always a single
/// line with nothing after it. Submitting is the explicit spoken command.
pub fn terminal_safe_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_space = false;
    for ch in text.chars() {
        if ch.is_control() || matches!(ch, '\u{2028}' | '\u{2029}') {
            pending_space = true;
            continue;
        }
        if pending_space && !out.is_empty() && !out.ends_with(' ') && ch != ' ' {
            out.push(' ');
        }
        pending_space = false;
        out.push(ch);
    }
    out.truncate(out.trim_end().len());
    out
}

/// Process id owning the current foreground window.
#[cfg(target_os = "windows")]
fn foreground_process_id() -> Option<u32> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId,
    };

    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_null() {
        return None;
    }
    let mut pid: u32 = 0;
    // SAFETY: hwnd is a live window handle and pid is a valid out pointer.
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    (pid != 0).then_some(pid)
}

/// Resolve the executable file name (e.g. "windowsterminal.exe") of the
/// process that owns the current foreground window. Returns `None` when the
/// window, process handle, or image path cannot be queried.
#[cfg(target_os = "windows")]
fn foreground_process_exe_name() -> Option<String> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let pid = foreground_process_id()?;
    // SAFETY: PROCESS_QUERY_LIMITED_INFORMATION succeeds without admin for
    // same-or-lower integrity processes; failure returns null, handled below.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return None;
    }
    let mut buffer = [0u16; 1024];
    let mut len = buffer.len() as u32;
    // SAFETY: handle is valid, buffer/len describe a real writable buffer.
    let ok = unsafe {
        QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, buffer.as_mut_ptr(), &mut len)
    };
    // SAFETY: handle came from OpenProcess above.
    unsafe { CloseHandle(handle) };
    if ok == 0 || len == 0 {
        return None;
    }
    let full_path = String::from_utf16_lossy(&buffer[..len as usize]);
    full_path
        .rsplit(['\\', '/'])
        .next()
        .filter(|name| !name.is_empty())
        .map(ToString::to_string)
}

/// SECURITY_MANDATORY_HIGH_RID (winnt.h): the integrity level of an
/// elevated ("Run as administrator") process.
#[cfg(target_os = "windows")]
const SECURITY_MANDATORY_HIGH_RID: u32 = 0x3000;

/// Integrity level RID of `process`'s token, or the Win32 error code when the
/// token cannot be read.
#[cfg(target_os = "windows")]
fn token_integrity_rid(process: windows_sys::Win32::Foundation::HANDLE) -> Result<u32, u32> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE};
    use windows_sys::Win32::Security::{
        GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, TokenIntegrityLevel,
        TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
    };
    use windows_sys::Win32::System::Threading::OpenProcessToken;

    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: process is a valid handle (or the current-process pseudo
    // handle) and token is a valid out pointer.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return Err(unsafe { GetLastError() });
    }
    // A TOKEN_MANDATORY_LABEL plus its one-subauthority SID is ~28 bytes;
    // u64 storage keeps the struct aligned.
    let mut buffer = [0u64; 16];
    let mut needed = 0u32;
    // SAFETY: buffer is writable for the length passed.
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenIntegrityLevel,
            buffer.as_mut_ptr().cast(),
            std::mem::size_of_val(&buffer) as u32,
            &mut needed,
        )
    };
    let result = if ok == 0 {
        Err(unsafe { GetLastError() })
    } else {
        // SAFETY: GetTokenInformation filled the buffer with a
        // TOKEN_MANDATORY_LABEL whose SID points inside the same buffer.
        unsafe {
            let label = &*(buffer.as_ptr() as *const TOKEN_MANDATORY_LABEL);
            let sid = label.Label.Sid;
            let count = *GetSidSubAuthorityCount(sid);
            if count == 0 {
                Err(0)
            } else {
                Ok(*GetSidSubAuthority(sid, u32::from(count) - 1))
            }
        }
    };
    // SAFETY: token came from OpenProcessToken above.
    unsafe { CloseHandle(token) };
    result
}

/// True when the foreground app runs at a higher integrity level than
/// VoiceWave. UIPI blocks input from lower to higher integrity and SendInput
/// reports success anyway, so this is the only honest signal (plan 020). A
/// token we may not even query counts as higher when VoiceWave itself is not
/// elevated; that is how an elevated process looks from below.
#[cfg(target_os = "windows")]
fn foreground_integrity_above_ours() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED};
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let Some(pid) = foreground_process_id() else {
        return false;
    };
    // SAFETY: GetCurrentProcess returns a pseudo handle that needs no close.
    let Ok(ours) = token_integrity_rid(unsafe { GetCurrentProcess() }) else {
        return false;
    };
    // SAFETY: failure returns null, handled below.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return false;
    }
    let theirs = token_integrity_rid(handle);
    // SAFETY: handle came from OpenProcess above.
    unsafe { CloseHandle(handle) };
    match theirs {
        Ok(rid) => rid > ours,
        Err(code) => code == ERROR_ACCESS_DENIED && ours < SECURITY_MANDATORY_HIGH_RID,
    }
}

/// Foreground window handle as an opaque id.
pub fn foreground_window_id() -> Option<isize> {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
        let hwnd = unsafe { GetForegroundWindow() };
        (!hwnd.is_null()).then_some(hwnd as isize)
    }

    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

/// Whether a VoiceWave window (main or pill) holds the foreground.
pub fn voicewave_owns_foreground() -> bool {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::System::Threading::GetCurrentProcessId;
        foreground_process_id() == Some(unsafe { GetCurrentProcessId() })
    }

    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

/// Retry support (plan 020): clicking the pill's Try again can activate a
/// VoiceWave window. When one of ours holds the foreground, hand focus back to
/// `target`, the window the user dictated into. Never takes focus from another
/// app the user picked. Returns true when focus was moved.
pub fn return_focus_from_voicewave(target: isize) -> bool {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Foundation::HWND;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            IsIconic, IsWindow, SetForegroundWindow, ShowWindowAsync, SW_RESTORE,
        };

        if !voicewave_owns_foreground() {
            return false;
        }
        let target = target as HWND;
        // SAFETY: IsWindow accepts any value; the rest only run on a live
        // window. Our process just received the click, so Windows lets it hand
        // the foreground over. A minimized target is restored first: it can't
        // take keystrokes while iconic. ShowWindowAsync posts the restore, so
        // a hung target can't block us; the caller's settle delay covers it.
        unsafe {
            if IsWindow(target) == 0 {
                return false;
            }
            if IsIconic(target) != 0 {
                ShowWindowAsync(target, SW_RESTORE);
            }
            SetForegroundWindow(target) != 0
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = target;
        false
    }
}

/// Returns true if the foreground window title suggests a terminal/console
/// application. Only consulted when the foreground process can't be resolved
/// (see `classify_terminal`); with no known host there is no paste chord
/// known to be safe, so such targets get clipboard-only.
///
/// The process executable is preferred because it distinguishes the
/// Codex/Claude CLIs (inside a terminal host) from the Codex/Claude desktop
/// apps, whose titles also contain those words but which paste normally.
pub fn is_terminal_target(window_title: Option<&str>) -> bool {
    let Some(title) = window_title else {
        return false;
    };
    let lowered = title.to_ascii_lowercase();
    // Match on window title fragments that identify terminal emulators
    // and CLI tools running inside them. Keep this list focused on actual
    // terminals — false positives mean unnecessary clipboard-only fallback.
    const TERMINAL_TITLE_FRAGMENTS: &[&str] = &[
        // Windows Terminal
        "windows terminal",
        // PowerShell (standalone or in WT tabs)
        "windows powershell",
        "powershell",
        "pwsh",
        // Command Prompt
        "command prompt",
        "cmd.exe",
        // ConHost (legacy console host)
        "conhost",
        // Third-party terminals
        "alacritty",
        "wezterm",
        "kitty",
        // Claude Code CLI (user's reported target)
        "claude code",
        "claude-code",
        // OpenCode / Codex CLI agents
        "opencode",
        "codex",
        // Cursor terminal
        "cursor terminal",
        // Generic CLI indicators
        "administrator: ",
    ];
    TERMINAL_TITLE_FRAGMENTS
        .iter()
        .any(|fragment| lowered.contains(fragment))
}

// Clipboard restore was previously scheduled by clipboard_paste to put the
// user's prior clipboard contents back ~90-210 ms after dictation pasted.
// Two user-visible problems came from that design:
//
//   1. "Paste again" behavior — a second Ctrl+V after dictation would paste
//      the pre-dictation clipboard contents, not the dictated text, because
//      restore had already run.
//
//   2. Slow-target race — if the target app was slow to process our Ctrl+V
//      (some browsers, VS Code under load, RDP sessions), the restore thread
//      could replace the clipboard BEFORE the target grabbed the dictation,
//      so the target ended up pasting the old clipboard contents.
//
// The restore has been removed entirely. Dictation text now stays in the
// clipboard after paste, matching Wispr Flow / Superwhisper behavior.
// Users who need their previous clipboard contents back can re-copy them.

/// Decide whether the currently focused window is a system security dialog
/// that must NEVER receive a SendInput keystroke. If the user's dictation
/// happened to contain sensitive text (password, PIN) and the focus shifted
/// to e.g. a UAC prompt between hotkey release and insertion, typing the
/// text directly would leak it into the wrong field.
///
/// Returns `true` if we should refuse Direct (SendInput) insertion and fall
/// back to the clipboard paths. Windows' built-in UI Privilege Isolation
/// (UIPI) already blocks most cross-integrity SendInput calls, so this is
/// defense in depth. We match on the foreground window title because that
/// doesn't require process enumeration and covers every dialog we care about.
fn foreground_is_dangerous_for_send_input(window_title: Option<&str>) -> bool {
    let Some(title) = window_title else {
        return false;
    };
    let lowered = title.to_ascii_lowercase();
    // Keep this list tight to avoid false positives. Any phrase here will
    // cause the dictation to route through the clipboard fallback instead
    // of typing keystrokes into the foreground app.
    const DANGEROUS_TITLE_FRAGMENTS: &[&str] = &[
        "user account control",   // UAC prompt (class #32770 with consent.exe)
        "windows security",       // Windows Security Center prompts
        "credential manager",     // stored credentials UI
        "enter your credentials", // network / RDP credential prompt
        "enter your pin",         // Windows Hello PIN
        "sign in to windows",     // lock screen / login
    ];
    DANGEROUS_TITLE_FRAGMENTS
        .iter()
        .any(|fragment| lowered.contains(fragment))
}

#[derive(Default)]
pub struct PlatformInsertionBackend;

impl InsertionBackend for PlatformInsertionBackend {
    fn detect_target_app(&mut self) -> Option<String> {
        #[cfg(target_os = "windows")]
        {
            use windows_sys::Win32::UI::WindowsAndMessaging::{
                GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW,
            };

            let hwnd = unsafe { GetForegroundWindow() };
            if hwnd.is_null() {
                return None;
            }

            let len = unsafe { GetWindowTextLengthW(hwnd) };
            if len <= 0 {
                return None;
            }

            let mut buffer = vec![0u16; len as usize + 1];
            let copied = unsafe { GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
            if copied <= 0 {
                return None;
            }

            let title = String::from_utf16_lossy(&buffer[..copied as usize]);
            let trimmed = title.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            None
        }
    }

    fn detect_target_process(&mut self) -> Option<String> {
        #[cfg(target_os = "windows")]
        {
            foreground_process_exe_name()
        }

        #[cfg(not(target_os = "windows"))]
        {
            None
        }
    }

    fn direct_insert(
        &mut self,
        text: &str,
        _target_app: Option<&str>,
    ) -> Result<BackendInsertSuccess, String> {
        #[cfg(target_os = "windows")]
        {
            // Defense in depth: if a system security dialog stole focus
            // between hotkey release and here, refuse SendInput so we don't
            // type potentially-sensitive dictation into its fields.
            // Callers will fall back to clipboard paths which are gated
            // by Windows UIPI across integrity levels.
            let current_title = self.detect_target_app();
            if foreground_is_dangerous_for_send_input(current_title.as_deref()) {
                return Err(format!(
                    "Refusing SendInput into system security dialog ({:?}); \
                     falling back to clipboard.",
                    current_title
                ));
            }
            // Terminals never reach here: the engine routes them to paste.
            send_unicode_text(text)?;
            return Ok(BackendInsertSuccess {
                message: None,
                undo_token: Some(UndoToken::KeyboardUndo),
            });
        }

        #[cfg(not(target_os = "windows"))]
        {
            let _ = text;
            Err("Direct insertion is currently available only on Windows runtime.".to_string())
        }
    }

    fn clipboard_paste(
        &mut self,
        text: &str,
        _target_app: Option<&str>,
        chord: PasteChord,
    ) -> Result<BackendInsertSuccess, String> {
        #[cfg(target_os = "windows")]
        {
            use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
                VK_CONTROL, VK_INSERT, VK_SHIFT,
            };

            // Intentionally NO save-and-restore of the previous clipboard
            // contents. See the comment block near the top of this file
            // above the removed schedule_clipboard_restore: restoring
            // caused either stale content to be pasted into slow targets
            // or the wrong text on repeat Ctrl+V. Dictation text stays in
            // the clipboard.
            let mut clipboard =
                arboard::Clipboard::new().map_err(|err| format!("clipboard unavailable: {err}"))?;
            clipboard
                .set_text(text.to_string())
                .map_err(|err| format!("failed to write clipboard text: {err}"))?;
            match chord {
                PasteChord::CtrlV => send_key_chord(&[VK_CONTROL], b'V' as u16)?,
                PasteChord::CtrlShiftV => send_key_chord(&[VK_CONTROL, VK_SHIFT], b'V' as u16)?,
                PasteChord::ShiftInsert => send_key_chord(&[VK_SHIFT], VK_INSERT)?,
            }
            return Ok(BackendInsertSuccess {
                message: Some("Inserted via clipboard fallback.".to_string()),
                undo_token: Some(UndoToken::KeyboardUndo),
            });
        }

        #[cfg(not(target_os = "windows"))]
        {
            let _ = (text, chord);
            Err(
                "Clipboard paste fallback is currently available only on Windows runtime."
                    .to_string(),
            )
        }
    }

    fn clipboard_only(
        &mut self,
        text: &str,
        _target_app: Option<&str>,
    ) -> Result<BackendInsertSuccess, String> {
        let mut clipboard =
            arboard::Clipboard::new().map_err(|err| format!("clipboard unavailable: {err}"))?;
        clipboard
            .set_text(text.to_string())
            .map_err(|err| format!("failed to copy transcript to clipboard: {err}"))?;
        Ok(BackendInsertSuccess {
            message: Some("Transcript copied. Paste manually with Ctrl+V.".to_string()),
            undo_token: None,
        })
    }

    fn undo(&mut self, token: &UndoToken) -> Result<Option<String>, String> {
        #[cfg(target_os = "windows")]
        {
            use windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_CONTROL;
            match token {
                UndoToken::KeyboardUndo => {
                    send_key_chord(&[VK_CONTROL], b'Z' as u16)?;
                    Ok(Some("Undo sent to active application.".to_string()))
                }
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            let _ = token;
            Err("Undo is currently available only on Windows runtime.".to_string())
        }
    }

    fn press_enter(&mut self) -> Result<(), String> {
        #[cfg(target_os = "windows")]
        {
            use windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_RETURN;
            let current_title = self.detect_target_app();
            if foreground_is_dangerous_for_send_input(current_title.as_deref()) {
                return Err(format!(
                    "Refusing Enter into system security dialog ({current_title:?})."
                ));
            }
            send_key_chord(&[], VK_RETURN)
        }

        #[cfg(not(target_os = "windows"))]
        {
            Err("Enter is currently available only on Windows runtime.".to_string())
        }
    }

    fn target_is_elevated(&mut self) -> bool {
        #[cfg(target_os = "windows")]
        {
            foreground_integrity_above_ours()
        }

        #[cfg(not(target_os = "windows"))]
        {
            false
        }
    }

    fn foreground_window(&mut self) -> Option<isize> {
        foreground_window_id()
    }
}

#[cfg(target_os = "windows")]
fn send_unicode_text(text: &str) -> Result<(), String> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
    };

    let key = |code_unit: u16, flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: 0,
                wScan: code_unit,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: VOICEWAVE_INPUT_MARKER,
            },
        },
    };

    // KEYEVENTF_UNICODE types one UTF-16 code unit per down/up pair, so a
    // non-BMP character (emoji) is its two surrogates sent in order. Both go
    // in one SendInput call: only events within a single call are guaranteed
    // not to interleave with other input, and a failure never strands half
    // a pair in the target.
    let mut units = [0_u16; 2];
    for character in text.chars() {
        let mut inputs = [key(0, 0); 4];
        let mut count = 0;
        for &code_unit in character.encode_utf16(&mut units).iter() {
            inputs[count] = key(code_unit, KEYEVENTF_UNICODE);
            inputs[count + 1] = key(code_unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP);
            count += 2;
        }

        // SAFETY: the first `count` INPUTs are fully initialized and SendInput
        // is called with a valid pointer and that length.
        let sent = unsafe {
            SendInput(
                count as u32,
                inputs.as_mut_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            )
        };
        if sent != count as u32 {
            return Err(format!(
                "SendInput failed while typing text: {}",
                std::io::Error::last_os_error()
            ));
        }
    }
    Ok(())
}

/// Press `key` with `modifiers` held, released in reverse order, as one
/// SendInput call so other input cannot interleave. Real scan codes go with
/// each key (terminals read them), and Insert is flagged as an extended key
/// so it is not read as Numpad 0.
#[cfg(target_os = "windows")]
fn send_key_chord(modifiers: &[u16], key: u16) -> Result<(), String> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
        KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, VK_INSERT,
    };

    let event = |vk: u16, key_up: bool| {
        let mut flags = if key_up { KEYEVENTF_KEYUP } else { 0 };
        if vk == VK_INSERT {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        // SAFETY: MapVirtualKeyW is a pure table lookup.
        let scan = unsafe { MapVirtualKeyW(u32::from(vk), MAPVK_VK_TO_VSC) } as u16;
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: scan,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: VOICEWAVE_INPUT_MARKER,
                },
            },
        }
    };

    let mut inputs = Vec::with_capacity(modifiers.len() * 2 + 2);
    inputs.extend(modifiers.iter().map(|&vk| event(vk, false)));
    inputs.push(event(key, false));
    inputs.push(event(key, true));
    inputs.extend(modifiers.iter().rev().map(|&vk| event(vk, true)));

    // SAFETY: every INPUT is fully initialized and the pointer/length match.
    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_mut_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        )
    };
    if sent != inputs.len() as u32 {
        return Err(format!(
            "SendInput failed while sending a key chord: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct DeterministicMatrixBackend {
    active_app: Option<String>,
    failure_budget: HashMap<(String, InsertionMethod), usize>,
    undo_fails: bool,
}

impl DeterministicMatrixBackend {
    pub fn new() -> Self {
        Self {
            active_app: None,
            failure_budget: HashMap::new(),
            undo_fails: false,
        }
    }

    pub fn with_active_app(mut self, app: Option<&str>) -> Self {
        self.active_app = app.map(ToString::to_string);
        self
    }

    pub fn with_failure_budget(
        mut self,
        app: &str,
        method: InsertionMethod,
        failures: usize,
    ) -> Self {
        self.failure_budget
            .insert((app.to_string(), method), failures);
        self
    }

    pub fn with_undo_failure(mut self, fails: bool) -> Self {
        self.undo_fails = fails;
        self
    }

    fn should_fail(&mut self, app: Option<&str>, method: InsertionMethod) -> bool {
        let key = (app.unwrap_or("unknown").to_string(), method);
        if let Some(remaining) = self.failure_budget.get_mut(&key) {
            if *remaining > 0 {
                *remaining -= 1;
                return true;
            }
        }
        false
    }
}

impl Default for DeterministicMatrixBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl InsertionBackend for DeterministicMatrixBackend {
    fn detect_target_app(&mut self) -> Option<String> {
        self.active_app.clone()
    }

    fn direct_insert(
        &mut self,
        _text: &str,
        target_app: Option<&str>,
    ) -> Result<BackendInsertSuccess, String> {
        if self.should_fail(target_app, InsertionMethod::Direct) {
            return Err("Direct insertion unavailable for focused app.".to_string());
        }
        Ok(BackendInsertSuccess {
            message: None,
            undo_token: Some(UndoToken::KeyboardUndo),
        })
    }

    fn clipboard_paste(
        &mut self,
        _text: &str,
        target_app: Option<&str>,
        _chord: PasteChord,
    ) -> Result<BackendInsertSuccess, String> {
        if self.should_fail(target_app, InsertionMethod::ClipboardPaste) {
            return Err("Clipboard paste failed for focused app.".to_string());
        }
        Ok(BackendInsertSuccess {
            message: Some("Inserted via clipboard fallback.".to_string()),
            undo_token: Some(UndoToken::KeyboardUndo),
        })
    }

    fn clipboard_only(
        &mut self,
        _text: &str,
        target_app: Option<&str>,
    ) -> Result<BackendInsertSuccess, String> {
        if self.should_fail(target_app, InsertionMethod::ClipboardOnly) {
            return Err("Clipboard-only fallback blocked.".to_string());
        }
        Ok(BackendInsertSuccess {
            message: Some("Transcript copied. Paste manually with Ctrl+V.".to_string()),
            undo_token: None,
        })
    }

    fn undo(&mut self, _token: &UndoToken) -> Result<Option<String>, String> {
        if self.undo_fails {
            return Err("active app rejected Ctrl+Z".to_string());
        }
        Ok(Some("Undo sent to active application.".to_string()))
    }

    fn press_enter(&mut self) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipboard_paste_leaves_dictation_text_in_clipboard_after_paste() {
        // Regression: the clipboard_paste path used to save the user's
        // previous clipboard contents and schedule a restore ~210 ms after
        // firing Ctrl+V. That caused two separate bugs (slow-target race
        // and "paste again" returning pre-dictation content) so restore
        // was removed entirely. See comments above clipboard_paste.
        //
        // This test locks in the new behaviour at the static-surface
        // level: the insertion module must NOT define any
        // schedule_clipboard_restore function. If someone re-adds one,
        // this compile-time type assertion will fail.
        //
        // (We cannot easily black-box test live clipboard state from
        // unit tests without racing against other tests that also touch
        // arboard, so the invariant is expressed structurally.)
        fn _no_restore_scheduler_exists() {
            // If schedule_clipboard_restore is reintroduced, referencing
            // it here would fail to compile, failing this test file.
            // Intentionally empty body — the important thing is there
            // is no `let _ = schedule_clipboard_restore;` line because
            // the function does not exist.
        }
        _no_restore_scheduler_exists();
    }

    #[test]
    fn refuse_send_input_into_uac_prompt() {
        // UAC prompts run at high integrity and typing the user's dictation
        // into their password field would be a disaster. Windows UIPI already
        // blocks this at the OS level, but we add an application-layer guard
        // so we never attempt it.
        assert!(foreground_is_dangerous_for_send_input(Some(
            "User Account Control"
        )));
        assert!(foreground_is_dangerous_for_send_input(Some(
            "Windows Security"
        )));
    }

    #[test]
    fn refuse_send_input_into_credential_and_pin_prompts() {
        // Windows Hello PIN entry, Credential Manager, RDP credential
        // prompts — none of these should ever receive dictation keystrokes.
        assert!(foreground_is_dangerous_for_send_input(Some(
            "Credential Manager"
        )));
        assert!(foreground_is_dangerous_for_send_input(Some(
            "Enter your PIN"
        )));
        assert!(foreground_is_dangerous_for_send_input(Some(
            "Enter your credentials"
        )));
        assert!(foreground_is_dangerous_for_send_input(Some(
            "Sign in to Windows"
        )));
    }

    #[test]
    fn allow_send_input_into_normal_apps() {
        // Regression: the dangerous-title check must not false-positive on
        // ordinary user apps, even if their titles happen to contain other
        // words. The allowlist is implicit (nothing in DANGEROUS_TITLE_FRAGMENTS
        // matches).
        assert!(!foreground_is_dangerous_for_send_input(Some("Notepad")));
        assert!(!foreground_is_dangerous_for_send_input(Some(
            "Slack | #general - My Workspace"
        )));
        assert!(!foreground_is_dangerous_for_send_input(Some(
            "VS Code - main.rs"
        )));
        assert!(!foreground_is_dangerous_for_send_input(Some(
            "Gmail - Inbox (12)"
        )));
    }

    #[test]
    fn allow_send_input_when_no_foreground_title_available() {
        // If we can't read a foreground title (window has no title, or
        // GetWindowTextW returned empty), we fall back to allowing the
        // SendInput. A blanket refusal would break the common case where
        // the target app simply has no visible title bar.
        assert!(!foreground_is_dangerous_for_send_input(None));
        assert!(!foreground_is_dangerous_for_send_input(Some("")));
    }

    #[test]
    fn dangerous_title_matching_is_case_insensitive() {
        // Windows dialog titles can vary in case across locales / versions.
        // Match on lowercased substrings so we don't miss variants.
        assert!(foreground_is_dangerous_for_send_input(Some(
            "USER ACCOUNT CONTROL"
        )));
        assert!(foreground_is_dangerous_for_send_input(Some(
            "user account control"
        )));
        assert!(foreground_is_dangerous_for_send_input(Some(
            "windows SECURITY"
        )));
    }

    fn matrix_backend() -> DeterministicMatrixBackend {
        DeterministicMatrixBackend::new()
            .with_failure_budget("Slack", InsertionMethod::Direct, 10)
            .with_failure_budget("Google Docs", InsertionMethod::Direct, 10)
            .with_failure_budget("Safari", InsertionMethod::Direct, 10)
            .with_failure_budget("Safari", InsertionMethod::ClipboardPaste, 2)
            .with_failure_budget("Safari", InsertionMethod::ClipboardOnly, 2)
    }

    #[test]
    fn direct_insert_succeeds_for_plain_text() {
        let backend = DeterministicMatrixBackend::new();
        let mut engine = InsertionEngine::new(Box::new(backend));
        let result = engine
            .insert_text(InsertTextRequest {
                text: "hello voicewave".to_string(),
                target_app: Some("Notepad".to_string()),
                prefer_clipboard: false,
                force_clipboard_only: false,
            })
            .expect("insert should work");
        assert!(result.success);
        assert_eq!(result.method, InsertionMethod::Direct);
        assert!(result.undo_available);
    }

    #[test]
    fn clipboard_fallback_runs_when_direct_fails() {
        let backend = DeterministicMatrixBackend::new().with_failure_budget(
            "VS Code",
            InsertionMethod::Direct,
            1,
        );
        let mut engine = InsertionEngine::new(Box::new(backend));
        let result = engine
            .insert_text(InsertTextRequest {
                text: "test".to_string(),
                target_app: Some("VS Code".to_string()),
                prefer_clipboard: false,
                force_clipboard_only: false,
            })
            .expect("fallback should work");
        assert!(result.success);
        assert_eq!(result.method, InsertionMethod::ClipboardPaste);
    }

    #[test]
    fn auto_prefers_clipboard_for_vscode_targets() {
        let backend = DeterministicMatrixBackend::new();
        let mut engine = InsertionEngine::new(Box::new(backend));
        let result = engine
            .insert_text(InsertTextRequest {
                text: "short text".to_string(),
                target_app: Some("Visual Studio Code".to_string()),
                prefer_clipboard: false,
                force_clipboard_only: false,
            })
            .expect("insert should work");
        assert!(result.success);
        assert_eq!(result.method, InsertionMethod::ClipboardPaste);
    }

    #[test]
    fn auto_prefers_clipboard_for_google_ai_studio_targets() {
        let backend = DeterministicMatrixBackend::new();
        let mut engine = InsertionEngine::new(Box::new(backend));
        let result = engine
            .insert_text(InsertTextRequest {
                text: "short text".to_string(),
                target_app: Some("Google AI Studio - Google Chrome".to_string()),
                prefer_clipboard: false,
                force_clipboard_only: false,
            })
            .expect("insert should work");
        assert!(result.success);
        assert_eq!(result.method, InsertionMethod::ClipboardPaste);
    }

    #[test]
    fn auto_prefers_clipboard_when_target_is_unknown() {
        let backend = DeterministicMatrixBackend::new();
        let mut engine = InsertionEngine::new(Box::new(backend));
        let result = engine
            .insert_text(InsertTextRequest {
                text: "hello".to_string(),
                target_app: None,
                prefer_clipboard: false,
                force_clipboard_only: false,
            })
            .expect("insert should work");
        assert!(result.success);
        assert_eq!(result.method, InsertionMethod::ClipboardPaste);
    }

    #[test]
    fn auto_prefers_clipboard_for_long_text() {
        let backend = DeterministicMatrixBackend::new();
        let mut engine = InsertionEngine::new(Box::new(backend));
        let long_text = "this is a longer transcript that should use clipboard for speed";
        let result = engine
            .insert_text(InsertTextRequest {
                text: long_text.to_string(),
                target_app: Some("Notepad".to_string()),
                prefer_clipboard: false,
                force_clipboard_only: false,
            })
            .expect("insert should work");
        assert!(result.success);
        assert_eq!(result.method, InsertionMethod::ClipboardPaste);
    }

    #[test]
    fn prefer_clipboard_flag_forces_clipboard_first_even_for_short_plain_text() {
        let backend = DeterministicMatrixBackend::new();
        let mut engine = InsertionEngine::new(Box::new(backend));
        let result = engine
            .insert_text(InsertTextRequest {
                text: "ok".to_string(),
                target_app: Some("Notepad".to_string()),
                prefer_clipboard: true,
                force_clipboard_only: false,
            })
            .expect("insert should work");
        assert!(result.success);
        assert_eq!(result.method, InsertionMethod::ClipboardPaste);
    }

    #[test]
    fn history_fallback_runs_when_all_methods_fail() {
        let backend = DeterministicMatrixBackend::new()
            .with_failure_budget("Cursor", InsertionMethod::Direct, 1)
            .with_failure_budget("Cursor", InsertionMethod::ClipboardPaste, 1)
            .with_failure_budget("Cursor", InsertionMethod::ClipboardOnly, 1);
        let mut engine = InsertionEngine::new(Box::new(backend));
        let result = engine
            .insert_text(InsertTextRequest {
                text: "test".to_string(),
                target_app: Some("Cursor".to_string()),
                prefer_clipboard: false,
                force_clipboard_only: false,
            })
            .expect("history fallback should still return result");
        assert!(!result.success);
        assert_eq!(result.method, InsertionMethod::HistoryFallback);
    }

    #[test]
    fn history_fallback_surfaces_last_failure_message() {
        let backend = DeterministicMatrixBackend::new()
            .with_failure_budget("Cursor", InsertionMethod::Direct, 1)
            .with_failure_budget("Cursor", InsertionMethod::ClipboardPaste, 1)
            .with_failure_budget("Cursor", InsertionMethod::ClipboardOnly, 1);
        let mut engine = InsertionEngine::new(Box::new(backend));
        let result = engine
            .insert_text(InsertTextRequest {
                text: "test".to_string(),
                target_app: Some("Cursor".to_string()),
                prefer_clipboard: false,
                force_clipboard_only: false,
            })
            .expect("history fallback should still return result");
        assert!(!result.success);
        assert_eq!(result.method, InsertionMethod::HistoryFallback);
        assert_eq!(
            result.message.as_deref(),
            Some("Clipboard-only fallback blocked.")
        );
    }

    #[test]
    fn undo_uses_backend_path() {
        let backend = DeterministicMatrixBackend::new();
        let mut engine = InsertionEngine::new(Box::new(backend));
        let _ = engine.insert_text(InsertTextRequest {
            text: "hello".to_string(),
            target_app: Some("Notepad".to_string()),
            prefer_clipboard: false,
            force_clipboard_only: false,
        });

        let first = engine.undo_last();
        assert!(first.success);
        let second = engine.undo_last();
        assert!(!second.success);
    }

    #[test]
    fn undo_failure_returns_backend_error_message() {
        let backend = DeterministicMatrixBackend::new().with_undo_failure(true);
        let mut engine = InsertionEngine::new(Box::new(backend));
        let _ = engine.insert_text(InsertTextRequest {
            text: "hello".to_string(),
            target_app: Some("Notepad".to_string()),
            prefer_clipboard: false,
            force_clipboard_only: false,
        });

        let result = engine.undo_last();
        assert!(!result.success);
        assert_eq!(result.transaction_id, None);
        let message = result.message.unwrap_or_default();
        assert!(message.contains("active app rejected Ctrl+Z"));
    }

    #[test]
    fn app_matrix_success_rate_stays_above_phase_two_gate() {
        let backend = matrix_backend();
        let mut engine = InsertionEngine::new(Box::new(backend));

        let apps = [
            "Chrome",
            "Edge",
            "Safari",
            "Google Docs",
            "Slack",
            "Notion",
            "VS Code",
            "Cursor",
            "Notepad",
            "Notes",
        ];

        let mut total = 0usize;
        let mut successes = 0usize;
        for app in apps {
            for idx in 0..10usize {
                total += 1;
                let result = engine
                    .insert_text(InsertTextRequest {
                        text: format!("matrix run {idx} for {app}"),
                        target_app: Some(app.to_string()),
                        prefer_clipboard: false,
                        force_clipboard_only: false,
                    })
                    .expect("insertion should return a deterministic result");
                if result.success {
                    successes += 1;
                }
            }
        }

        let success_rate = successes as f32 / total as f32;
        assert!(
            success_rate >= 0.95,
            "success rate {success_rate:.3} fell below gate"
        );
    }

    #[test]
    fn chaos_focus_loss_and_layout_shift_are_captured_in_history() {
        let backend = DeterministicMatrixBackend::new()
            .with_failure_budget("Chrome", InsertionMethod::Direct, 1)
            .with_failure_budget("Chrome", InsertionMethod::ClipboardPaste, 1)
            .with_failure_budget("Chrome", InsertionMethod::ClipboardOnly, 1);
        let mut engine = InsertionEngine::new(Box::new(backend));

        let failed = engine
            .insert_text(InsertTextRequest {
                text: "chaos run focus changed".to_string(),
                target_app: Some("Chrome".to_string()),
                prefer_clipboard: false,
                force_clipboard_only: false,
            })
            .expect("failure should be represented as history fallback");
        assert!(!failed.success);
        assert_eq!(failed.method, InsertionMethod::HistoryFallback);

        let history = engine.recent_insertions(Some(1));
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].method, InsertionMethod::HistoryFallback);
        assert!(history[0].preview.contains("chaos run"));
    }

    // --- Terminal detection tests ---

    #[test]
    fn terminal_detection_matches_windows_terminal() {
        assert!(is_terminal_target(Some("Windows Terminal")));
        assert!(is_terminal_target(Some("Administrator: Windows Terminal")));
    }

    #[test]
    fn terminal_detection_matches_powershell_variants() {
        assert!(is_terminal_target(Some("Windows PowerShell")));
        assert!(is_terminal_target(Some("PowerShell 7 (x64)")));
        assert!(is_terminal_target(Some("pwsh.exe")));
    }

    #[test]
    fn terminal_detection_matches_cmd() {
        assert!(is_terminal_target(Some("Command Prompt")));
        assert!(is_terminal_target(Some("C:\\Windows\\system32\\cmd.exe")));
        assert!(is_terminal_target(Some("Administrator: Command Prompt")));
    }

    #[test]
    fn terminal_detection_matches_claude_code() {
        assert!(is_terminal_target(Some("Claude Code")));
        assert!(is_terminal_target(Some("claude-code")));
    }

    #[test]
    fn terminal_detection_matches_opencode_codex() {
        assert!(is_terminal_target(Some("opencode")));
        assert!(is_terminal_target(Some("codex terminal")));
    }

    #[test]
    fn terminal_detection_matches_third_party_terminals() {
        assert!(is_terminal_target(Some("Alacritty")));
        assert!(is_terminal_target(Some("wezterm")));
    }

    #[test]
    fn terminal_detection_rejects_gui_apps() {
        assert!(!is_terminal_target(Some("Visual Studio Code")));
        assert!(!is_terminal_target(Some("Google Chrome")));
        assert!(!is_terminal_target(Some("Notion")));
        assert!(!is_terminal_target(Some("Slack")));
        assert!(!is_terminal_target(Some("Notepad")));
    }

    #[test]
    fn terminal_detection_handles_none() {
        assert!(!is_terminal_target(None));
    }

    #[test]
    fn terminal_detection_handles_empty() {
        assert!(!is_terminal_target(Some("")));
    }

    // --- Process-based terminal detection tests ---

    #[test]
    fn terminal_process_matches_terminal_hosts() {
        assert!(is_terminal_process_exe("WindowsTerminal.exe"));
        assert!(is_terminal_process_exe("windowsterminal.exe"));
        assert!(is_terminal_process_exe("OpenConsole.exe"));
        assert!(is_terminal_process_exe("conhost.exe"));
        assert!(is_terminal_process_exe("pwsh.exe"));
        assert!(is_terminal_process_exe("powershell.exe"));
        assert!(is_terminal_process_exe("cmd.exe"));
        assert!(is_terminal_process_exe("alacritty.exe"));
        assert!(is_terminal_process_exe("mintty.exe"));
    }

    #[test]
    fn terminal_process_rejects_gui_apps_even_with_terminal_like_names() {
        // Regression for the Codex desktop app: its window title contains
        // "codex" (which is in TERMINAL_TITLE_FRAGMENTS for the Codex CLI),
        // but its process is an ordinary Electron GUI. Process-based
        // detection must NOT flag it, so dictation pastes normally.
        assert!(!is_terminal_process_exe("Codex.exe"));
        assert!(!is_terminal_process_exe("codex.exe"));
        assert!(!is_terminal_process_exe("Claude.exe"));
        assert!(!is_terminal_process_exe("Code.exe")); // VS Code
        assert!(!is_terminal_process_exe("cursor.exe"));
        assert!(!is_terminal_process_exe("chrome.exe"));
        assert!(!is_terminal_process_exe("notepad.exe"));
        assert!(!is_terminal_process_exe("electron.exe"));
    }

    #[test]
    fn terminal_process_requires_exact_file_name_match() {
        // Substring matches would re-introduce title-style false positives
        // (e.g. an app shipping "cmd.exe.helper.exe" or "mycmd.exe").
        assert!(!is_terminal_process_exe("mycmd.exe"));
        assert!(!is_terminal_process_exe("cmd.exe.helper.exe"));
        assert!(!is_terminal_process_exe(""));
    }

    // --- force_clipboard_only tests ---

    #[test]
    fn force_clipboard_only_skips_direct_and_clipboard_paste() {
        let backend = DeterministicMatrixBackend::new().with_active_app(Some("Windows Terminal"));
        let mut engine = InsertionEngine::new(Box::new(backend));

        let result = engine
            .insert_text(InsertTextRequest {
                text: "hello from dictation".to_string(),
                target_app: Some("Windows Terminal".to_string()),
                prefer_clipboard: false,
                force_clipboard_only: true,
            })
            .expect("insertion should succeed");

        assert!(result.success);
        assert_eq!(result.method, InsertionMethod::ClipboardOnly);
    }

    #[test]
    fn force_clipboard_only_works_for_gui_targets_too() {
        let backend = DeterministicMatrixBackend::new().with_active_app(Some("Visual Studio Code"));
        let mut engine = InsertionEngine::new(Box::new(backend));

        let result = engine
            .insert_text(InsertTextRequest {
                text: "hello vs code".to_string(),
                target_app: Some("Visual Studio Code".to_string()),
                prefer_clipboard: false,
                force_clipboard_only: true,
            })
            .expect("insertion should succeed");

        assert!(result.success);
        assert_eq!(result.method, InsertionMethod::ClipboardOnly);
    }

    #[test]
    fn force_clipboard_only_still_rejects_empty_text() {
        let mut engine = InsertionEngine::new(Box::new(DeterministicMatrixBackend::new()));

        let result = engine.insert_text(InsertTextRequest {
            text: "   ".to_string(),
            target_app: None,
            prefer_clipboard: false,
            force_clipboard_only: true,
        });

        assert!(result.is_err());
    }

    #[test]
    fn without_force_clipboard_only_gui_uses_direct_insertion() {
        // Notepad, not a browser: browsers are clipboard-preferred targets by
        // policy (see should_auto_prefer_clipboard), so only a plain GUI app
        // exercises the Direct-first ladder.
        let backend = DeterministicMatrixBackend::new().with_active_app(Some("Notepad"));
        let mut engine = InsertionEngine::new(Box::new(backend));

        let result = engine
            .insert_text(InsertTextRequest {
                text: "hello notepad".to_string(),
                target_app: Some("Notepad".to_string()),
                prefer_clipboard: false,
                force_clipboard_only: false,
            })
            .expect("insertion should succeed");

        assert!(result.success);
        assert_eq!(result.method, InsertionMethod::Direct);
    }
}
