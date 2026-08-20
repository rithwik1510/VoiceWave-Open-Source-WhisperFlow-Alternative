use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex as StdMutex, OnceLock,
    },
};
use tokio::sync::mpsc::UnboundedSender;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyConfig {
    pub toggle: String,
    pub push_to_talk: String,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            toggle: "Ctrl+Alt+X".to_string(),
            push_to_talk: "Ctrl+Windows".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HotkeySnapshot {
    pub config: HotkeyConfig,
    pub conflicts: Vec<String>,
    pub registration_supported: bool,
    pub registration_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum HotkeyAction {
    ToggleDictation,
    PushToTalk,
    CancelDictation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum HotkeyPhase {
    Pressed,
    Released,
    Triggered,
}

#[derive(Debug, thiserror::Error)]
pub enum HotkeyError {
    #[error("hotkey '{field}' cannot be empty")]
    EmptyBinding { field: &'static str },
    #[error("hotkey '{field}' has invalid token '{token}'")]
    InvalidToken { field: &'static str, token: String },
    #[error("hotkey '{field}' must include one non-modifier key")]
    MissingMainKey { field: &'static str },
    #[error("toggle and push-to-talk hotkeys conflict")]
    Conflict,
    #[error("global hotkey runtime failed: {0}")]
    Runtime(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeySignal {
    pub action: HotkeyAction,
    pub phase: HotkeyPhase,
}

#[derive(Debug, Clone)]
struct ParsedHotkey {
    ctrl: bool,
    shift: bool,
    alt: bool,
    super_key: bool,
    modifier_only: bool,
    main_vk: u16,
}

#[derive(Debug)]
struct HotkeyEdgeTracker {
    toggle: ParsedHotkey,
    push_to_talk: ParsedHotkey,
    keys_down: HashSet<u16>,
    escape_down: bool,
    escape_owned: bool,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct HotkeyEventOutcome {
    signals: Vec<HotkeySignal>,
    consume: bool,
}

impl HotkeyEdgeTracker {
    fn new(config: &HotkeyConfig) -> Result<Self, HotkeyError> {
        Ok(Self {
            toggle: parse_combo("toggle", &config.toggle)?,
            push_to_talk: parse_combo("pushToTalk", &config.push_to_talk)?,
            keys_down: HashSet::new(),
            escape_down: false,
            escape_owned: false,
        })
    }

    fn key_event(&mut self, vk: u16, pressed: bool) -> Vec<HotkeySignal> {
        self.handle_key_event(vk, pressed, false).signals
    }

    fn handle_key_event(
        &mut self,
        vk: u16,
        pressed: bool,
        dictation_active: bool,
    ) -> HotkeyEventOutcome {
        if vk == VK_ESCAPE_ {
            if pressed {
                if self.escape_down {
                    return HotkeyEventOutcome {
                        consume: self.escape_owned,
                        ..HotkeyEventOutcome::default()
                    };
                }

                self.escape_down = true;
                self.escape_owned = dictation_active;
                if self.escape_owned {
                    return HotkeyEventOutcome {
                        signals: vec![HotkeySignal {
                            action: HotkeyAction::CancelDictation,
                            phase: HotkeyPhase::Triggered,
                        }],
                        consume: true,
                    };
                }
            } else {
                if !self.escape_down {
                    return HotkeyEventOutcome::default();
                }
                self.escape_down = false;
                let consume = self.escape_owned;
                self.escape_owned = false;
                return HotkeyEventOutcome {
                    consume,
                    ..HotkeyEventOutcome::default()
                };
            }

            return HotkeyEventOutcome::default();
        }

        let push_was_active = parsed_is_active(&self.push_to_talk, &self.keys_down);

        if pressed {
            // Auto-repeat must not produce additional toggle or push edges.
            if !self.keys_down.insert(vk) {
                return HotkeyEventOutcome::default();
            }
        } else if !self.keys_down.remove(&vk) {
            return HotkeyEventOutcome::default();
        }

        let mut signals = Vec::with_capacity(2);
        if pressed
            && !self.toggle.modifier_only
            && vk == self.toggle.main_vk
            && parsed_is_active(&self.toggle, &self.keys_down)
        {
            signals.push(HotkeySignal {
                action: HotkeyAction::ToggleDictation,
                phase: HotkeyPhase::Triggered,
            });
        }

        let push_is_active = parsed_is_active(&self.push_to_talk, &self.keys_down);
        if !push_was_active && push_is_active {
            signals.push(HotkeySignal {
                action: HotkeyAction::PushToTalk,
                phase: HotkeyPhase::Pressed,
            });
        } else if push_was_active && !push_is_active {
            signals.push(HotkeySignal {
                action: HotkeyAction::PushToTalk,
                phase: HotkeyPhase::Released,
            });
        }
        HotkeyEventOutcome {
            signals,
            consume: false,
        }
    }
}

fn parsed_is_active(parsed: &ParsedHotkey, keys_down: &HashSet<u16>) -> bool {
    let ctrl_down = key_set_contains_any(keys_down, &[VK_CONTROL_, VK_LCONTROL_, VK_RCONTROL_]);
    let shift_down = key_set_contains_any(keys_down, &[VK_SHIFT_, VK_LSHIFT_, VK_RSHIFT_]);
    let alt_down = key_set_contains_any(keys_down, &[VK_MENU_, VK_LMENU_, VK_RMENU_]);
    let super_down = key_set_contains_any(keys_down, &[VK_LWIN_, VK_RWIN_]);

    if parsed.ctrl != ctrl_down
        || parsed.shift != shift_down
        || parsed.alt != alt_down
        || parsed.super_key != super_down
    {
        return false;
    }
    parsed.modifier_only || keys_down.contains(&parsed.main_vk)
}

fn key_set_contains_any(keys_down: &HashSet<u16>, candidates: &[u16]) -> bool {
    candidates.iter().any(|vk| keys_down.contains(vk))
}

// Virtual-key values are stable Win32 ABI constants. Keeping the small set
// here makes the edge tracker platform-neutral and directly unit-testable.
const VK_CONTROL_: u16 = 0x11;
const VK_SHIFT_: u16 = 0x10;
const VK_MENU_: u16 = 0x12;
const VK_LCONTROL_: u16 = 0xA2;
const VK_RCONTROL_: u16 = 0xA3;
const VK_LSHIFT_: u16 = 0xA0;
const VK_RSHIFT_: u16 = 0xA1;
const VK_LMENU_: u16 = 0xA4;
const VK_RMENU_: u16 = 0xA5;
const VK_LWIN_: u16 = 0x5B;
const VK_RWIN_: u16 = 0x5C;
const VK_ESCAPE_: u16 = 0x1B;

pub struct HotkeyRuntime {
    dictation_active: Arc<AtomicBool>,
    #[cfg(target_os = "windows")]
    thread_id: u32,
    #[cfg(target_os = "windows")]
    thread: Option<std::thread::JoinHandle<()>>,
}

impl HotkeyRuntime {
    pub fn start(
        config: HotkeyConfig,
        sender: UnboundedSender<HotkeySignal>,
    ) -> Result<Self, HotkeyError> {
        #[cfg(target_os = "windows")]
        {
            start_windows_hotkey_runtime(config, sender)
        }

        #[cfg(not(target_os = "windows"))]
        {
            let _ = (config, sender);
            Err(HotkeyError::Runtime(
                "event-driven global hotkeys are currently available only on Windows".to_string(),
            ))
        }
    }

    /// Controls whether Escape is owned by VoiceWave's global keyboard hook.
    ///
    /// An Escape press that begins while active is consumed through its
    /// matching key-up, even if cancellation clears this flag first. Presses
    /// that begin while inactive are always passed through unchanged.
    pub fn set_dictation_active(&self, active: bool) {
        self.dictation_active.store(active, Ordering::Release);
    }
}

#[cfg(target_os = "windows")]
impl Drop for HotkeyRuntime {
    fn drop(&mut self) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(target_os = "windows")]
struct HookDispatch {
    tracker: HotkeyEdgeTracker,
    sender: UnboundedSender<HotkeySignal>,
    dictation_active: Arc<AtomicBool>,
}

#[cfg(target_os = "windows")]
static HOOK_DISPATCH: OnceLock<StdMutex<Option<HookDispatch>>> = OnceLock::new();

/// Session-global claim on the WH_KEYBOARD_LL hook.
///
/// `HOOK_DISPATCH` only guarantees one hook per *process*. Two full VoiceWave
/// processes would install two hooks and insert every dictation twice, so the
/// claim has to live in the kernel. Held for the life of the hook thread and
/// released in `Drop`, which runs as that thread unwinds.
#[cfg(target_os = "windows")]
struct GlobalHookMutex(windows_sys::Win32::Foundation::HANDLE);

#[cfg(target_os = "windows")]
impl Drop for GlobalHookMutex {
    fn drop(&mut self) {
        use windows_sys::Win32::{Foundation::CloseHandle, System::Threading::ReleaseMutex};
        // SAFETY: `self.0` is a live mutex handle created (and owned) by the
        // same thread that drops this guard.
        unsafe {
            let _ = ReleaseMutex(self.0);
            let _ = CloseHandle(self.0);
        }
    }
}

/// Claims the session-global hook mutex, or reports who already owns it.
///
/// No `Global\` prefix: per-logon-session scope is exactly right, since a
/// keyboard hook only ever sees its own interactive session anyway.
#[cfg(target_os = "windows")]
fn acquire_global_hook_mutex() -> Result<GlobalHookMutex, String> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS},
        System::Threading::CreateMutexW,
    };

    let name: Vec<u16> = "VoiceWave-hotkey-hook"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: `name` is a valid nul-terminated UTF-16 buffer that outlives the
    // call; a null security descriptor gives the default ACL.
    let handle = unsafe { CreateMutexW(std::ptr::null(), 1, name.as_ptr()) };
    if handle.is_null() {
        // SAFETY: GetLastError only reads this thread's last-error slot.
        let code = unsafe { GetLastError() };
        return Err(format!(
            "CreateMutexW for the keyboard hook failed (error {code})"
        ));
    }
    // SAFETY: as above.
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        // CreateMutexW still hands back a (non-owning) handle in this case.
        // SAFETY: `handle` is a live handle we are done with.
        unsafe {
            let _ = CloseHandle(handle);
        }
        return Err("another VoiceWave instance already owns the keyboard hook".to_string());
    }
    Ok(GlobalHookMutex(handle))
}

#[cfg(target_os = "windows")]
fn start_windows_hotkey_runtime(
    config: HotkeyConfig,
    sender: UnboundedSender<HotkeySignal>,
) -> Result<HotkeyRuntime, HotkeyError> {
    use std::sync::mpsc;
    use std::time::Duration;

    let tracker = HotkeyEdgeTracker::new(&config)?;
    let dictation_active = Arc::new(AtomicBool::new(false));
    let hook_dictation_active = Arc::clone(&dictation_active);
    let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<u32, String>>(1);
    let thread = std::thread::Builder::new()
        .name("voicewave-hotkey-events".to_string())
        .spawn(move || windows_hotkey_thread(tracker, sender, hook_dictation_active, ready_tx))
        .map_err(|err| HotkeyError::Runtime(format!("failed to spawn hook thread: {err}")))?;

    match ready_rx.recv_timeout(Duration::from_secs(3)) {
        Ok(Ok(thread_id)) => Ok(HotkeyRuntime {
            dictation_active,
            thread_id,
            thread: Some(thread),
        }),
        Ok(Err(message)) => {
            let _ = thread.join();
            Err(HotkeyError::Runtime(message))
        }
        Err(err) => Err(HotkeyError::Runtime(format!(
            "hotkey hook startup timed out: {err}"
        ))),
    }
}

#[cfg(target_os = "windows")]
fn windows_hotkey_thread(
    tracker: HotkeyEdgeTracker,
    sender: UnboundedSender<HotkeySignal>,
    dictation_active: Arc<AtomicBool>,
    ready: std::sync::mpsc::SyncSender<Result<u32, String>>,
) {
    use windows_sys::Win32::{
        System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId},
        UI::WindowsAndMessaging::{
            DispatchMessageW, GetMessageW, PeekMessageW, SetWindowsHookExW, TranslateMessage,
            UnhookWindowsHookEx, MSG, PM_NOREMOVE, WH_KEYBOARD_LL,
        },
    };

    let thread_id = unsafe { GetCurrentThreadId() };
    // Force creation of the thread message queue before publishing readiness,
    // so Drop can always wake GetMessageW with WM_QUIT.
    let mut message: MSG = unsafe { std::mem::zeroed() };
    unsafe {
        let _ = PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_NOREMOVE);
    }

    // Claim the system-wide hook slot BEFORE installing anything. `_hook_claim`
    // stays alive for the rest of this function, so the mutex is released only
    // once the message loop has exited and the hook is torn down.
    let _hook_claim = match acquire_global_hook_mutex() {
        Ok(claim) => claim,
        Err(message) => {
            let _ = ready.send(Err(message));
            return;
        }
    };

    let module = unsafe { GetModuleHandleW(std::ptr::null()) };
    if module.is_null() {
        let _ = ready.send(Err("GetModuleHandleW failed for hotkey hook".to_string()));
        return;
    }
    let hook =
        unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(low_level_keyboard_proc), module, 0) };
    if hook.is_null() {
        let _ = ready.send(Err("SetWindowsHookExW(WH_KEYBOARD_LL) failed".to_string()));
        return;
    }

    let dispatch = HOOK_DISPATCH.get_or_init(|| StdMutex::new(None));
    {
        let Ok(mut slot) = dispatch.lock() else {
            unsafe {
                let _ = UnhookWindowsHookEx(hook);
            }
            let _ = ready.send(Err("hotkey hook state lock is poisoned".to_string()));
            return;
        };
        if slot.is_some() {
            unsafe {
                let _ = UnhookWindowsHookEx(hook);
            }
            let _ = ready.send(Err("a hotkey hook is already running".to_string()));
            return;
        }
        *slot = Some(HookDispatch {
            tracker,
            sender,
            dictation_active,
        });
    }
    let _ = ready.send(Ok(thread_id));

    loop {
        let result = unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) };
        if result <= 0 {
            break;
        }
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }

    if let Ok(mut slot) = dispatch.lock() {
        *slot = None;
    }
    unsafe {
        let _ = UnhookWindowsHookEx(hook);
    }
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn low_level_keyboard_proc(
    code: i32,
    wparam: usize,
    lparam: isize,
) -> isize {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, KBDLLHOOKSTRUCT, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
    };

    if code >= 0 {
        let pressed = wparam == WM_KEYDOWN as usize || wparam == WM_SYSKEYDOWN as usize;
        let released = wparam == WM_KEYUP as usize || wparam == WM_SYSKEYUP as usize;
        if pressed || released {
            let event = &*(lparam as *const KBDLLHOOKSTRUCT);
            if let Some(dispatch) = HOOK_DISPATCH.get() {
                if let Ok(mut state) = dispatch.lock() {
                    if let Some(state) = state.as_mut() {
                        let dictation_active = state.dictation_active.load(Ordering::Acquire);
                        let outcome = state.tracker.handle_key_event(
                            event.vkCode as u16,
                            pressed,
                            dictation_active,
                        );
                        for signal in outcome.signals {
                            let _ = state.sender.send(signal);
                        }
                        if outcome.consume {
                            return 1;
                        }
                    }
                }
            }
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

#[derive(Debug, Clone)]
pub struct HotkeyManager {
    config: HotkeyConfig,
    parsed_toggle: ParsedHotkey,
    parsed_push_to_talk: ParsedHotkey,
    registration_supported: bool,
    registration_error: Option<String>,
}

impl HotkeyManager {
    pub fn new(config: HotkeyConfig) -> Result<Self, HotkeyError> {
        validate_config(&config)?;
        let parsed_toggle = parse_combo("toggle", &config.toggle)?;
        let parsed_push_to_talk = parse_combo("pushToTalk", &config.push_to_talk)?;
        let (registration_supported, registration_error) = platform_registration_status();
        Ok(Self {
            config,
            parsed_toggle,
            parsed_push_to_talk,
            registration_supported,
            registration_error,
        })
    }

    pub fn config(&self) -> HotkeyConfig {
        self.config.clone()
    }

    pub fn snapshot(&self) -> HotkeySnapshot {
        HotkeySnapshot {
            config: self.config(),
            conflicts: detect_conflicts(&self.config),
            registration_supported: self.registration_supported,
            registration_error: self.registration_error.clone(),
        }
    }

    pub fn update_config(&mut self, config: HotkeyConfig) -> Result<HotkeySnapshot, HotkeyError> {
        validate_config(&config)?;
        self.parsed_toggle = parse_combo("toggle", &config.toggle)?;
        self.parsed_push_to_talk = parse_combo("pushToTalk", &config.push_to_talk)?;
        self.config = config;
        Ok(self.snapshot())
    }

    pub fn is_action_pressed(&self, action: HotkeyAction) -> bool {
        if !self.registration_supported {
            return false;
        }
        match action {
            HotkeyAction::ToggleDictation => is_parsed_pressed(&self.parsed_toggle),
            HotkeyAction::PushToTalk => is_parsed_pressed(&self.parsed_push_to_talk),
            HotkeyAction::CancelDictation => false,
        }
    }
}

fn validate_config(config: &HotkeyConfig) -> Result<(), HotkeyError> {
    normalize_combo("toggle", &config.toggle)?;
    normalize_combo("pushToTalk", &config.push_to_talk)?;
    if detect_conflicts(config)
        .iter()
        .any(|c| c == "duplicateBinding")
    {
        return Err(HotkeyError::Conflict);
    }
    Ok(())
}

fn detect_conflicts(config: &HotkeyConfig) -> Vec<String> {
    let toggle = normalize_combo("toggle", &config.toggle).ok();
    let push = normalize_combo("pushToTalk", &config.push_to_talk).ok();

    let mut conflicts = Vec::new();
    if toggle.is_some() && toggle == push {
        conflicts.push("duplicateBinding".to_string());
    }
    conflicts
}

fn normalize_combo(field: &'static str, combo: &str) -> Result<String, HotkeyError> {
    if combo.trim().is_empty() {
        return Err(HotkeyError::EmptyBinding { field });
    }

    let modifier_aliases = [
        ("CTRL", "CTRL"),
        ("CONTROL", "CTRL"),
        ("SHIFT", "SHIFT"),
        ("ALT", "ALT"),
        ("OPTION", "ALT"),
        ("WIN", "SUPER"),
        ("WINDOWS", "SUPER"),
        ("META", "SUPER"),
        ("CMD", "SUPER"),
        ("SUPER", "SUPER"),
    ];

    let mut modifiers = HashSet::new();
    let mut main_keys = Vec::new();

    for raw in combo.split('+') {
        let token = raw.trim().to_uppercase();
        if token.is_empty() {
            return Err(HotkeyError::InvalidToken {
                field,
                token: raw.to_string(),
            });
        }

        if let Some((_, normalized)) = modifier_aliases.iter().find(|(alias, _)| *alias == token) {
            modifiers.insert(*normalized);
            continue;
        }

        if token == "SPACE"
            || token
                .strip_prefix('F')
                .and_then(|suffix| suffix.parse::<u8>().ok())
                .is_some_and(|n| (1..=24).contains(&n))
            || (token.len() == 1 && token.chars().all(|c| c.is_ascii_alphanumeric()))
        {
            main_keys.push(token);
            continue;
        }

        return Err(HotkeyError::InvalidToken { field, token });
    }

    if main_keys.is_empty() {
        if field == "pushToTalk"
            && modifiers.contains("CTRL")
            && modifiers.contains("SUPER")
            && modifiers.len() == 2
        {
            let mut ordered_modifiers = modifiers.into_iter().collect::<Vec<_>>();
            ordered_modifiers.sort_unstable();
            return Ok(ordered_modifiers.join("+"));
        }
        return Err(HotkeyError::MissingMainKey { field });
    }
    if main_keys.len() != 1 {
        return Err(HotkeyError::MissingMainKey { field });
    }

    let mut ordered_modifiers = modifiers.into_iter().collect::<Vec<_>>();
    ordered_modifiers.sort_unstable();
    ordered_modifiers.push(main_keys[0].as_str());
    Ok(ordered_modifiers.join("+"))
}

fn parse_combo(field: &'static str, combo: &str) -> Result<ParsedHotkey, HotkeyError> {
    let normalized = normalize_combo(field, combo)?;
    let mut ctrl = false;
    let mut shift = false;
    let mut alt = false;
    let mut super_key = false;
    let mut main_vk: Option<u16> = None;

    for token in normalized.split('+') {
        match token {
            "CTRL" => ctrl = true,
            "SHIFT" => shift = true,
            "ALT" => alt = true,
            "SUPER" => super_key = true,
            "SPACE" => main_vk = Some(vk_space()),
            _ if token.starts_with('F') => {
                let number = token.trim_start_matches('F').parse::<u8>().map_err(|_| {
                    HotkeyError::InvalidToken {
                        field,
                        token: token.to_string(),
                    }
                })?;
                if !(1..=24).contains(&number) {
                    return Err(HotkeyError::InvalidToken {
                        field,
                        token: token.to_string(),
                    });
                }
                main_vk = Some(vk_f(number));
            }
            _ if token.len() == 1 => {
                let byte = token.as_bytes()[0];
                main_vk = Some(byte as u16);
            }
            _ => {
                return Err(HotkeyError::InvalidToken {
                    field,
                    token: token.to_string(),
                })
            }
        }
    }

    let modifier_only = main_vk.is_none();
    let main_vk = if modifier_only {
        0
    } else {
        main_vk.ok_or(HotkeyError::MissingMainKey { field })?
    };
    Ok(ParsedHotkey {
        ctrl,
        shift,
        alt,
        super_key,
        modifier_only,
        main_vk,
    })
}

fn platform_registration_status() -> (bool, Option<String>) {
    #[cfg(target_os = "windows")]
    {
        return (true, None);
    }

    #[cfg(not(target_os = "windows"))]
    {
        (
            false,
            Some("Global hotkeys are currently implemented for Windows runtime.".to_string()),
        )
    }
}

fn is_parsed_pressed(parsed: &ParsedHotkey) -> bool {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
            GetAsyncKeyState, VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU,
            VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
        };

        let ctrl_down = key_down_any(&[VK_CONTROL as u16, VK_LCONTROL as u16, VK_RCONTROL as u16]);
        let shift_down = key_down_any(&[VK_SHIFT as u16, VK_LSHIFT as u16, VK_RSHIFT as u16]);
        let alt_down = key_down_any(&[VK_MENU as u16, VK_LMENU as u16, VK_RMENU as u16]);
        let super_down = key_down(VK_LWIN as u16) || key_down(VK_RWIN as u16);

        if parsed.ctrl != ctrl_down
            || parsed.shift != shift_down
            || parsed.alt != alt_down
            || parsed.super_key != super_down
        {
            return false;
        }
        if parsed.modifier_only {
            return true;
        }

        // SAFETY: GetAsyncKeyState is thread-safe for querying current key state.
        let state = unsafe { GetAsyncKeyState(parsed.main_vk as i32) };
        (state as u16 & 0x8000) != 0
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = parsed;
        false
    }
}

#[cfg(target_os = "windows")]
fn key_down(vk: u16) -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    // SAFETY: GetAsyncKeyState is thread-safe for querying current key state.
    let state = unsafe { GetAsyncKeyState(vk as i32) };
    (state as u16 & 0x8000) != 0
}

#[cfg(target_os = "windows")]
fn key_down_any(vks: &[u16]) -> bool {
    vks.iter().copied().any(key_down)
}

fn vk_space() -> u16 {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_SPACE;
        return VK_SPACE as u16;
    }

    #[cfg(not(target_os = "windows"))]
    {
        0x20
    }
}

fn vk_f(number: u8) -> u16 {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_F1;
        return (VK_F1 as u16) + (number as u16 - 1);
    }

    #[cfg(not(target_os = "windows"))]
    {
        0x70 + (number as u16 - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracker() -> HotkeyEdgeTracker {
        HotkeyEdgeTracker::new(&HotkeyConfig::default()).expect("default tracker")
    }

    #[test]
    fn default_hotkeys_are_valid() {
        let manager =
            HotkeyManager::new(HotkeyConfig::default()).expect("default config should be valid");
        let snapshot = manager.snapshot();
        assert!(snapshot.conflicts.is_empty());
        #[cfg(target_os = "windows")]
        assert!(snapshot.registration_supported);
    }

    #[test]
    fn duplicate_hotkeys_are_rejected() {
        let result = HotkeyManager::new(HotkeyConfig {
            toggle: "Ctrl+Alt+X".to_string(),
            push_to_talk: "Ctrl+Alt+X".to_string(),
        });
        assert!(matches!(result, Err(HotkeyError::Conflict)));
    }

    #[test]
    fn invalid_token_is_rejected() {
        let result = HotkeyManager::new(HotkeyConfig {
            toggle: "Ctrl+Banana".to_string(),
            push_to_talk: "Ctrl+Windows".to_string(),
        });
        assert!(matches!(result, Err(HotkeyError::InvalidToken { .. })));
    }

    #[test]
    fn push_to_talk_supports_modifier_only_ctrl_windows_combo() {
        let parsed = parse_combo("pushToTalk", "Ctrl+Windows").expect("combo should parse");
        assert!(parsed.ctrl);
        assert!(parsed.super_key);
        assert!(parsed.modifier_only);
    }

    #[test]
    fn parse_function_key_combo() {
        let parsed = parse_combo("toggle", "Ctrl+F13").expect("combo should parse");
        assert!(parsed.ctrl);
        assert_eq!(parsed.main_vk, vk_f(13));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_event_runtime_installs_and_shuts_down_cleanly() {
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let runtime = HotkeyRuntime::start(HotkeyConfig::default(), sender)
            .expect("Windows low-level keyboard hook should install");
        drop(runtime);
    }

    #[test]
    fn toggle_edge_fires_once_even_for_a_short_press_and_ignores_repeat() {
        let mut tracker = tracker();
        assert!(tracker.key_event(VK_LCONTROL_, true).is_empty());
        assert!(tracker.key_event(VK_LMENU_, true).is_empty());
        assert_eq!(
            tracker.key_event(b'X' as u16, true),
            vec![HotkeySignal {
                action: HotkeyAction::ToggleDictation,
                phase: HotkeyPhase::Triggered,
            }]
        );
        assert!(tracker.key_event(b'X' as u16, true).is_empty());
        assert!(tracker.key_event(b'X' as u16, false).is_empty());
    }

    #[test]
    fn escape_passes_through_unchanged_when_dictation_is_idle() {
        let mut tracker = tracker();

        let down = tracker.handle_key_event(VK_ESCAPE_, true, false);
        assert!(down.signals.is_empty());
        assert!(!down.consume);

        let repeat = tracker.handle_key_event(VK_ESCAPE_, true, false);
        assert!(repeat.signals.is_empty());
        assert!(!repeat.consume);

        let up = tracker.handle_key_event(VK_ESCAPE_, false, false);
        assert!(up.signals.is_empty());
        assert!(!up.consume);
    }

    #[test]
    fn active_escape_emits_once_and_consumes_auto_repeat() {
        let mut tracker = tracker();

        assert_eq!(
            tracker.handle_key_event(VK_ESCAPE_, true, true),
            HotkeyEventOutcome {
                signals: vec![HotkeySignal {
                    action: HotkeyAction::CancelDictation,
                    phase: HotkeyPhase::Triggered,
                }],
                consume: true,
            }
        );
        assert_eq!(
            tracker.handle_key_event(VK_ESCAPE_, true, true),
            HotkeyEventOutcome {
                signals: Vec::new(),
                consume: true,
            }
        );
    }

    #[test]
    fn escape_release_preserves_press_ownership_across_state_changes() {
        let mut tracker = tracker();

        let down = tracker.handle_key_event(VK_ESCAPE_, true, true);
        assert!(down.consume);

        // The cancellation handler may clear active state before key-up. The
        // application must still not receive a release for a consumed press.
        let up_after_cancel = tracker.handle_key_event(VK_ESCAPE_, false, false);
        assert!(up_after_cancel.signals.is_empty());
        assert!(up_after_cancel.consume);

        // A subsequent idle press is not owned by VoiceWave.
        let idle_down = tracker.handle_key_event(VK_ESCAPE_, true, false);
        let idle_up = tracker.handle_key_event(VK_ESCAPE_, false, false);
        assert!(!idle_down.consume);
        assert!(!idle_up.consume);
    }

    #[test]
    fn escape_press_that_begins_idle_stays_pass_through_if_dictation_activates() {
        let mut tracker = tracker();

        assert!(!tracker.handle_key_event(VK_ESCAPE_, true, false).consume);
        assert!(!tracker.handle_key_event(VK_ESCAPE_, true, true).consume);
        assert!(!tracker.handle_key_event(VK_ESCAPE_, false, true).consume);
    }

    #[test]
    fn push_to_talk_emits_edges_when_modifiers_arrive_in_either_order() {
        for (first, second) in [(VK_LCONTROL_, VK_LWIN_), (VK_RWIN_, VK_RCONTROL_)] {
            let mut tracker = tracker();
            assert!(tracker.key_event(first, true).is_empty());
            assert_eq!(
                tracker.key_event(second, true),
                vec![HotkeySignal {
                    action: HotkeyAction::PushToTalk,
                    phase: HotkeyPhase::Pressed,
                }]
            );
            assert_eq!(
                tracker.key_event(second, false),
                vec![HotkeySignal {
                    action: HotkeyAction::PushToTalk,
                    phase: HotkeyPhase::Released,
                }]
            );
        }
    }

    #[test]
    fn rapid_push_press_release_is_never_debounced_away() {
        let mut tracker = tracker();
        assert!(tracker.key_event(VK_LCONTROL_, true).is_empty());
        assert_eq!(tracker.key_event(VK_LWIN_, true).len(), 1);
        assert_eq!(tracker.key_event(VK_LWIN_, false).len(), 1);
    }

    #[test]
    fn push_auto_repeat_and_release_bounce_do_not_duplicate_edges() {
        let mut tracker = tracker();
        assert!(tracker.key_event(VK_LCONTROL_, true).is_empty());
        assert_eq!(tracker.key_event(VK_LWIN_, true).len(), 1);
        assert!(tracker.key_event(VK_LWIN_, true).is_empty());
        assert_eq!(tracker.key_event(VK_LWIN_, false).len(), 1);
        assert!(tracker.key_event(VK_LWIN_, false).is_empty());
    }

    #[test]
    fn releasing_one_of_two_control_keys_keeps_push_session_active() {
        let mut tracker = tracker();
        assert!(tracker.key_event(VK_LCONTROL_, true).is_empty());
        assert!(tracker.key_event(VK_RCONTROL_, true).is_empty());
        assert_eq!(tracker.key_event(VK_LWIN_, true).len(), 1);
        assert!(tracker.key_event(VK_LCONTROL_, false).is_empty());
        assert_eq!(
            tracker.key_event(VK_LWIN_, false),
            vec![HotkeySignal {
                action: HotkeyAction::PushToTalk,
                phase: HotkeyPhase::Released,
            }]
        );
    }
}
