mod atomic_file;
pub mod audio;
pub mod benchmark;
pub mod billing;
pub mod cue;
pub mod diagnostics;
pub mod dictionary;
pub mod history;
pub mod hotkey;
pub mod inference;
pub mod insertion;
mod job_object;
pub mod model_manager;
pub mod permissions;
pub mod phase1;
#[cfg(feature = "desktop")]
mod pill_hit_target;
mod secure_store;
pub mod settings;
pub mod snippet;
pub mod stats;
#[cfg(feature = "desktop")]
mod support;
pub mod transcript;

#[cfg(feature = "desktop")]
pub mod state;

#[cfg(feature = "desktop")]
use audio::AudioQualityReport;
#[cfg(feature = "desktop")]
use benchmark::{BenchmarkRequest, BenchmarkRun, ModelRecommendation, RecommendationConstraints};
#[cfg(feature = "desktop")]
use billing::{CheckoutLaunchResult, EntitlementSnapshot, PortalLaunchResult};
#[cfg(feature = "desktop")]
use diagnostics::{DiagnosticsExportResult, DiagnosticsStatus};
#[cfg(feature = "desktop")]
use dictionary::{
    DictionaryExport, DictionaryImportSummary, DictionaryQueueItem, DictionaryReconcileResult,
    DictionarySyncRecord, DictionaryTerm,
};
#[cfg(feature = "desktop")]
use history::{
    HistoryExportPreset, HistoryExportResult, RetentionPolicy, SessionHistoryQuery,
    SessionHistoryRecord,
};
#[cfg(feature = "desktop")]
use hotkey::{HotkeyAction, HotkeyConfig, HotkeyPhase, HotkeySnapshot};
#[cfg(feature = "desktop")]
use inference::{kill_asr_workers, kill_faster_whisper_workers, llm_polish::kill_polish_worker};
#[cfg(feature = "desktop")]
use insertion::{InsertResult, InsertTextRequest, RecentInsertion, UndoResult};
#[cfg(feature = "desktop")]
use model_manager::{InstalledModel, ModelCatalogItem, ModelDownloadRequest, ModelStatus};
#[cfg(feature = "desktop")]
use permissions::PermissionSnapshot;
#[cfg(feature = "desktop")]
use pill_hit_target::PillHitTarget;
#[cfg(feature = "desktop")]
use settings::{
    AppProfileOverrides, CodeModeSettings, DomainPackId, FormatProfile, VoiceWaveSettings,
};
#[cfg(feature = "desktop")]
use snippet::{SnippetError, VoiceSnippet, VoiceSnippetReconcileResult, VoiceSnippetSyncRecord};
#[cfg(feature = "desktop")]
use state::{DictationControlMode, DictationMode, VoiceWaveController, VoiceWaveSnapshot};
#[cfg(feature = "desktop")]
use stats::StatsSummary;
#[cfg(feature = "desktop")]
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex as StdMutex,
};
#[cfg(feature = "desktop")]
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Listener, LogicalSize, Manager, PhysicalPosition, Position, Size, State, WebviewUrl,
    WebviewWindowBuilder, WindowEvent,
};

#[cfg(feature = "desktop")]
const PILL_WINDOW_LABEL: &str = "voicewave-pill";

#[cfg(feature = "desktop")]
const PILL_QUICK_CONTROLS_SPIKE_ENV: &str = "VOICEWAVE_PILL_QUICK_CONTROLS_SPIKE";

#[cfg(feature = "desktop")]
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SnippetCommandError {
    code: &'static str,
    message: String,
    retryable: bool,
}

#[cfg(feature = "desktop")]
impl From<SnippetError> for SnippetCommandError {
    fn from(error: SnippetError) -> Self {
        let code = match &error {
            SnippetError::EmptyTrigger
            | SnippetError::TriggerTooLong
            | SnippetError::InvalidTriggerCharacters
            | SnippetError::ReservedTrigger
            | SnippetError::EmptyExpansion
            | SnippetError::ExpansionTooLong
            | SnippetError::InvalidExpansionCharacters
            | SnippetError::InvalidSyncTimestamps
            | SnippetError::TombstoneContainsExpansion => "snippet-validation",
            SnippetError::DuplicateTrigger => "snippet-duplicate",
            SnippetError::NotFound(_) => "snippet-not-found",
            SnippetError::ActiveLimit => "snippet-active-limit",
            SnippetError::InvalidSyncIdentity => "snippet-identity-mismatch",
            SnippetError::AppData | SnippetError::Persistence(_) | SnippetError::Parse(_) => {
                "snippet-persistence"
            }
        };
        let retryable = matches!(
            &error,
            SnippetError::AppData | SnippetError::Persistence(_) | SnippetError::Parse(_)
        );
        Self {
            code,
            message: error.to_string(),
            retryable,
        }
    }
}
#[cfg(feature = "desktop")]
// Compact window dimensions are intentionally larger than the pill surface
// (52x20 idle, 96x36 listening) so the drop shadow can fully fade out
// instead of being clipped into a visible rectangular edge.
const PILL_WINDOW_COMPACT_WIDTH: f64 = 200.0;
#[cfg(feature = "desktop")]
const PILL_WINDOW_COMPACT_HEIGHT: f64 = 96.0;
#[cfg(feature = "desktop")]
const PILL_WINDOW_REVIEW_WIDTH: f64 = 500.0;
#[cfg(feature = "desktop")]
const PILL_WINDOW_REVIEW_HEIGHT: f64 = 160.0;
#[cfg(feature = "desktop")]
// Zero bottom margin: the idle pill rests flush against the screen edge and
// rises into view (CSS translateY) only while dictation is active.
const PILL_WINDOW_COMPACT_BOTTOM_MARGIN: f64 = 0.0;
#[cfg(feature = "desktop")]
const PILL_WINDOW_REVIEW_BOTTOM_MARGIN: f64 = 54.0;
#[cfg(feature = "desktop")]
// Notice mode (Dynamic Island expansion): wide enough for a one-line message
// plus detail, anchored to the same bottom edge as the compact pill so the
// capsule visibly grows upward instead of jumping to a new position.
const PILL_WINDOW_NOTICE_WIDTH: f64 = 560.0;
#[cfg(feature = "desktop")]
const PILL_WINDOW_NOTICE_HEIGHT: f64 = 180.0;
#[cfg(feature = "desktop")]
const PILL_WINDOW_NOTICE_BOTTOM_MARGIN: f64 = 0.0;
#[cfg(feature = "desktop")]
const PILL_WINDOW_NUDGE_X: i32 = -22;
#[cfg(feature = "desktop")]
const PILL_HIT_TARGET_WIDTH: f64 = 72.0;
#[cfg(feature = "desktop")]
const PILL_HIT_TARGET_HEIGHT: f64 = 20.0;
#[cfg(feature = "desktop")]
const PILL_WINDOW_SPIKE_QUICK_WIDTH: f64 = 340.0;
#[cfg(feature = "desktop")]
const PILL_WINDOW_SPIKE_QUICK_HEIGHT: f64 = 184.0;
#[cfg(feature = "desktop")]
const PILL_WINDOW_SPIKE_QUICK_BOTTOM_MARGIN: f64 = 12.0;
#[cfg(feature = "desktop")]
const TRAY_ID: &str = "voicewave-tray";
/// Argument the "Start with Windows" Run-key entry launches the app with.
#[cfg(feature = "desktop")]
const AUTOSTART_ARG: &str = "--autostart";
#[cfg(feature = "desktop")]
const TRAY_SHOW_ID: &str = "show";
#[cfg(feature = "desktop")]
const TRAY_QUICK_CONTROLS_ID: &str = "quick-controls";
#[cfg(feature = "desktop")]
const TRAY_QUIT_ID: &str = "quit";

#[cfg(feature = "desktop")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
enum PillQuickControlsInvocation {
    Mouse,
    Focused,
}

#[cfg(feature = "desktop")]
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PillQuickControlsSnapshot {
    enabled: bool,
    invocation: Option<PillQuickControlsInvocation>,
}

#[cfg(feature = "desktop")]
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PillQuickControlsEvent {
    open: bool,
    invocation: Option<PillQuickControlsInvocation>,
}

#[cfg(feature = "desktop")]
#[derive(Debug, Default)]
struct PillQuickControlsSpikeInner {
    invocation: Option<PillQuickControlsInvocation>,
    session_generation: u64,
    // HWND is process-local and pointer-sized. Store the integer value so the
    // managed state remains Send + Sync; it is validated with IsWindow before
    // every use and is never persisted across launches.
    prior_foreground_hwnd: Option<isize>,
}

#[cfg(feature = "desktop")]
#[derive(Debug)]
struct PillQuickControlsSpikeState {
    enabled: bool,
    inner: StdMutex<PillQuickControlsSpikeInner>,
}

#[cfg(feature = "desktop")]
#[derive(Debug)]
struct PillHitTargetState {
    target: StdMutex<Option<PillHitTarget>>,
    blockers: StdMutex<PillHitTargetBlockers>,
    desired_visible: AtomicBool,
}

#[cfg(feature = "desktop")]
impl Default for PillHitTargetState {
    fn default() -> Self {
        Self {
            target: StdMutex::new(None),
            blockers: StdMutex::new(PillHitTargetBlockers::default()),
            desired_visible: AtomicBool::new(false),
        }
    }
}

#[cfg(feature = "desktop")]
#[derive(Debug, Default)]
struct PillHitTargetBlockers {
    review: bool,
    notice: bool,
    lifecycle: bool,
}

#[cfg(feature = "desktop")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
enum PillWindowMode {
    Compact,
    Review,
    Notice,
}

#[cfg(feature = "desktop")]
impl PillQuickControlsSpikeState {
    fn from_env() -> Self {
        Self {
            enabled: env_flag_enabled(std::env::var(PILL_QUICK_CONTROLS_SPIKE_ENV).ok().as_deref()),
            inner: StdMutex::new(PillQuickControlsSpikeInner::default()),
        }
    }

    fn snapshot(&self) -> PillQuickControlsSnapshot {
        let invocation = self.inner.lock().ok().and_then(|inner| inner.invocation);
        PillQuickControlsSnapshot {
            enabled: self.enabled,
            invocation,
        }
    }

    fn focused_session_generation(&self) -> Option<u64> {
        self.inner.lock().ok().and_then(|inner| {
            (inner.invocation == Some(PillQuickControlsInvocation::Focused))
                .then_some(inner.session_generation)
        })
    }
}

#[cfg(feature = "desktop")]
fn env_flag_enabled(value: Option<&str>) -> bool {
    value.is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

#[cfg(feature = "desktop")]
#[derive(Clone)]
struct RuntimeContext {
    controller: Arc<VoiceWaveController>,
}

#[cfg(feature = "desktop")]
#[derive(Debug, thiserror::Error)]
enum AppError {
    #[error("{0}")]
    Controller(#[from] state::ControllerError),
}

#[cfg(feature = "desktop")]
impl From<AppError> for String {
    fn from(value: AppError) -> Self {
        value.to_string()
    }
}

#[cfg(feature = "desktop")]
fn ensure_pill_window(app: &tauri::AppHandle) -> Result<tauri::WebviewWindow, String> {
    if let Some(window) = app.get_webview_window(PILL_WINDOW_LABEL) {
        return Ok(window);
    }

    let builder =
        WebviewWindowBuilder::new(app, PILL_WINDOW_LABEL, WebviewUrl::App("pill.html".into()))
            .title("VoiceWave Pill")
            .inner_size(PILL_WINDOW_COMPACT_WIDTH, PILL_WINDOW_COMPACT_HEIGHT)
            .resizable(false)
            .maximizable(false)
            .minimizable(false)
            .closable(false)
            .focused(false)
            .visible(false)
            .transparent(true)
            .decorations(false)
            .always_on_top(true)
            .visible_on_all_workspaces(true)
            .skip_taskbar(true)
            .shadow(false);

    let window = builder
        .build()
        .map_err(|err| format!("failed to create floating pill window: {err}"))?;
    let focus_app = app.clone();
    let focus_window = window.clone();
    window.on_window_event(move |event| {
        if !matches!(event, WindowEvent::Focused(false)) {
            return;
        }
        let Some(generation) = focus_app
            .state::<PillQuickControlsSpikeState>()
            .focused_session_generation()
        else {
            return;
        };
        let app = focus_app.clone();
        let pill = focus_window.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(140)).await;
            let still_same_session = app
                .state::<PillQuickControlsSpikeState>()
                .focused_session_generation()
                == Some(generation);
            let still_unfocused = pill.is_focused().map(|focused| !focused).unwrap_or(true);
            if still_same_session && still_unfocused {
                if let Err(err) = dismiss_pill_quick_controls_spike_if_open(&app) {
                    tracing::warn!("failed to close unfocused Quick Controls: {err}");
                }
            }
        });
    });
    if let Err(err) = position_pill_window(
        app,
        &window,
        PILL_WINDOW_COMPACT_WIDTH,
        PILL_WINDOW_COMPACT_HEIGHT,
        PILL_WINDOW_COMPACT_BOTTOM_MARGIN,
    ) {
        tracing::warn!("initial pill positioning deferred: {err}");
    }
    Ok(window)
}

#[cfg(feature = "desktop")]
fn position_pill_window(
    app: &tauri::AppHandle,
    window: &tauri::WebviewWindow,
    width_logical: f64,
    height_logical: f64,
    bottom_margin_logical: f64,
) -> Result<(), String> {
    // Anchor the pill to the monitor the user is actually working on (cursor
    // position), not the main window's monitor: the main window is usually
    // minimized to tray, and on multi-monitor setups it can be parked on a
    // different display than the one being dictated into — which made the
    // pill appear "gone".
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|pos| app.monitor_from_point(pos.x, pos.y).ok().flatten())
        .or_else(|| {
            app.get_webview_window("main")
                .and_then(|main| main.current_monitor().ok().flatten())
        })
        .or_else(|| app.primary_monitor().ok().flatten());

    let monitor = monitor.ok_or_else(|| "no monitor available for floating pill".to_string())?;

    // work_area is in physical pixels; the pill dimensions are logical, so
    // scale them before mixing the two or the pill drifts off-center (and
    // sits too low) on scaled displays.
    let scale = monitor.scale_factor();
    let work_area = monitor.work_area();
    let width = (width_logical * scale).round() as i32;
    let height = (height_logical * scale).round() as i32;
    let margin = (bottom_margin_logical * scale).round() as i32;
    let center_offset_x = ((work_area.size.width as i32 - width) / 2).max(0);
    let bottom_offset_y = (work_area.size.height as i32 - height - margin).max(0);
    let nudge_x = ((PILL_WINDOW_NUDGE_X as f64) * scale).round() as i32;
    let x = work_area.position.x + center_offset_x + nudge_x;
    let y = work_area.position.y + bottom_offset_y;
    window
        .set_position(Position::Physical(PhysicalPosition::new(x, y)))
        .map_err(|err| format!("failed to position floating pill: {err}"))
}

#[cfg(feature = "desktop")]
fn pill_hit_target_bounds(app: &tauri::AppHandle) -> Result<(i32, i32, i32, i32), String> {
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|pos| app.monitor_from_point(pos.x, pos.y).ok().flatten())
        .or_else(|| {
            app.get_webview_window("main")
                .and_then(|main| main.current_monitor().ok().flatten())
        })
        .or_else(|| app.primary_monitor().ok().flatten())
        .ok_or_else(|| "no monitor available for pill hit target".to_string())?;
    let scale = monitor.scale_factor();
    let work_area = monitor.work_area();
    let width = (PILL_HIT_TARGET_WIDTH * scale).round() as i32;
    let height = (PILL_HIT_TARGET_HEIGHT * scale).round() as i32;
    let center_offset_x = ((work_area.size.width as i32 - width) / 2).max(0);
    let bottom_offset_y = (work_area.size.height as i32 - height).max(0);
    let nudge_x = ((PILL_WINDOW_NUDGE_X as f64) * scale).round() as i32;
    Ok((
        work_area.position.x + center_offset_x + nudge_x,
        work_area.position.y + bottom_offset_y,
        width,
        height,
    ))
}

#[cfg(feature = "desktop")]
fn pill_window_geometry(mode: PillWindowMode) -> (f64, f64, f64) {
    match mode {
        PillWindowMode::Compact => (
            PILL_WINDOW_COMPACT_WIDTH,
            PILL_WINDOW_COMPACT_HEIGHT,
            PILL_WINDOW_COMPACT_BOTTOM_MARGIN,
        ),
        PillWindowMode::Review => (
            PILL_WINDOW_REVIEW_WIDTH,
            PILL_WINDOW_REVIEW_HEIGHT,
            PILL_WINDOW_REVIEW_BOTTOM_MARGIN,
        ),
        PillWindowMode::Notice => (
            PILL_WINDOW_NOTICE_WIDTH,
            PILL_WINDOW_NOTICE_HEIGHT,
            PILL_WINDOW_NOTICE_BOTTOM_MARGIN,
        ),
    }
}

#[cfg(feature = "desktop")]
fn show_pill_hit_target(app: &tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<PillHitTargetState>();
    if !state.desired_visible.load(Ordering::Acquire) {
        return Ok(());
    }
    if app
        .state::<PillQuickControlsSpikeState>()
        .snapshot()
        .invocation
        .is_some()
    {
        return Ok(());
    }
    let blocked = state
        .blockers
        .lock()
        .map_err(|_| "pill hit-target blocker state is poisoned".to_string())?;
    if blocked.review || blocked.notice || blocked.lifecycle {
        return Ok(());
    }
    drop(blocked);
    if foreground_is_fullscreen() {
        hide_pill_hit_target(app);
        return Ok(());
    }
    let (x, y, width, height) = pill_hit_target_bounds(app)?;
    let target = state
        .target
        .lock()
        .map_err(|_| "pill hit-target state is poisoned".to_string())?;
    if let Some(target) = target.as_ref() {
        target.show(x, y, width, height)?;
    }
    Ok(())
}

#[cfg(all(feature = "desktop", target_os = "windows"))]
fn foreground_is_fullscreen() -> bool {
    use windows_sys::Win32::{
        Foundation::RECT,
        Graphics::Gdi::{
            GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
        },
        UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowLongPtrW, GetWindowRect, GWL_STYLE, WS_CAPTION,
            WS_THICKFRAME,
        },
    };

    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_null() {
        return false;
    }
    let mut window_rect: RECT = unsafe { std::mem::zeroed() };
    if unsafe { GetWindowRect(hwnd, &mut window_rect) } == 0 {
        return false;
    }
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    if monitor.is_null() {
        return false;
    }
    let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
        return false;
    }

    let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) } as u32;
    let looks_like_normal_window = style & (WS_CAPTION | WS_THICKFRAME) != 0;
    fullscreen_geometry_matches(
        [
            window_rect.left,
            window_rect.top,
            window_rect.right,
            window_rect.bottom,
        ],
        [
            info.rcMonitor.left,
            info.rcMonitor.top,
            info.rcMonitor.right,
            info.rcMonitor.bottom,
        ],
        looks_like_normal_window,
    )
}

#[cfg(all(feature = "desktop", not(target_os = "windows")))]
fn foreground_is_fullscreen() -> bool {
    false
}

#[cfg(feature = "desktop")]
fn fullscreen_geometry_matches(
    window_rect: [i32; 4],
    monitor_rect: [i32; 4],
    has_normal_frame: bool,
) -> bool {
    const TOLERANCE: i32 = 2;
    let covers_monitor = window_rect[0] <= monitor_rect[0] + TOLERANCE
        && window_rect[1] <= monitor_rect[1] + TOLERANCE
        && window_rect[2] >= monitor_rect[2] - TOLERANCE
        && window_rect[3] >= monitor_rect[3] - TOLERANCE;
    covers_monitor && !has_normal_frame
}

#[cfg(feature = "desktop")]
fn set_pill_hit_target_blocker(
    app: &tauri::AppHandle,
    review: Option<bool>,
    notice: Option<bool>,
    lifecycle: Option<bool>,
) -> Result<(), String> {
    let state = app.state::<PillHitTargetState>();
    let mut blockers = state
        .blockers
        .lock()
        .map_err(|_| "pill hit-target blocker state is poisoned".to_string())?;
    if let Some(review) = review {
        blockers.review = review;
    }
    if let Some(notice) = notice {
        blockers.notice = notice;
    }
    if let Some(lifecycle) = lifecycle {
        blockers.lifecycle = lifecycle;
    }
    Ok(())
}

#[cfg(feature = "desktop")]
fn hide_pill_hit_target(app: &tauri::AppHandle) {
    let state = app.state::<PillHitTargetState>();
    if let Ok(target) = state.target.lock() {
        if let Some(target) = target.as_ref() {
            target.hide();
        }
    };
}

#[cfg(feature = "desktop")]
fn initialize_pill_hit_target(app: &tauri::AppHandle, visible: bool) -> Result<(), String> {
    if !app.state::<PillQuickControlsSpikeState>().enabled {
        return Ok(());
    }
    let click_app = app.clone();
    let target = PillHitTarget::spawn(move || {
        let spike = click_app.state::<PillQuickControlsSpikeState>();
        if !spike.enabled || spike.snapshot().invocation.is_some() {
            return;
        }
        hide_pill_hit_target(&click_app);
        if let Err(err) = apply_pill_quick_controls_spike_mode(
            &click_app,
            &spike,
            true,
            PillQuickControlsInvocation::Mouse,
        ) {
            tracing::warn!("native pill click failed to open Quick Controls: {err}");
            let _ = show_pill_hit_target(&click_app);
        }
    })?;
    *app.state::<PillHitTargetState>()
        .target
        .lock()
        .map_err(|_| "pill hit-target state is poisoned".to_string())? = Some(target);
    let monitor_app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            if let Err(err) = show_pill_hit_target(&monitor_app) {
                tracing::warn!("pill hit-target safety refresh failed: {err}");
            }
        }
    });
    if visible {
        show_pill_hit_target(app)?;
    }
    Ok(())
}

#[cfg(all(feature = "desktop", target_os = "windows"))]
fn set_spike_window_no_activate(
    window: &tauri::WebviewWindow,
    no_activate: bool,
) -> Result<(), String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE,
    };

    let hwnd = window
        .hwnd()
        .map_err(|err| format!("failed to get floating pill HWND: {err}"))?;
    let raw_hwnd = hwnd.0 as windows_sys::Win32::Foundation::HWND;
    let current = unsafe { GetWindowLongPtrW(raw_hwnd, GWL_EXSTYLE) };
    let no_activate_bit = WS_EX_NOACTIVATE as isize;
    let updated = if no_activate {
        current | no_activate_bit
    } else {
        current & !no_activate_bit
    };
    if updated != current {
        // SetWindowLongPtrW returns the previous value; a zero previous value
        // is valid, so there is no reliable error sentinel without clearing
        // GetLastError first. Read the style back and verify the exact bit.
        unsafe {
            SetWindowLongPtrW(raw_hwnd, GWL_EXSTYLE, updated);
        }
        let verified = unsafe { GetWindowLongPtrW(raw_hwnd, GWL_EXSTYLE) };
        if (verified & no_activate_bit != 0) != no_activate {
            return Err("failed to apply floating pill no-activate style".to_string());
        }
    }
    Ok(())
}

#[cfg(all(feature = "desktop", not(target_os = "windows")))]
fn set_spike_window_no_activate(
    _window: &tauri::WebviewWindow,
    _no_activate: bool,
) -> Result<(), String> {
    Err("pill Quick Controls spike is Windows-only".to_string())
}

#[cfg(all(feature = "desktop", target_os = "windows"))]
fn resize_and_position_spike_window_no_activate(
    app: &tauri::AppHandle,
    window: &tauri::WebviewWindow,
    width_logical: f64,
    height_logical: f64,
    bottom_margin_logical: f64,
) -> Result<(), String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_NOZORDER, SWP_SHOWWINDOW,
    };

    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|pos| app.monitor_from_point(pos.x, pos.y).ok().flatten())
        .or_else(|| {
            app.get_webview_window("main")
                .and_then(|main| main.current_monitor().ok().flatten())
        })
        .or_else(|| app.primary_monitor().ok().flatten())
        .ok_or_else(|| "no monitor available for floating pill".to_string())?;
    let scale = monitor.scale_factor();
    let work_area = monitor.work_area();
    let width = (width_logical * scale).round() as i32;
    let height = (height_logical * scale).round() as i32;
    let margin = (bottom_margin_logical * scale).round() as i32;
    let center_offset_x = ((work_area.size.width as i32 - width) / 2).max(0);
    let bottom_offset_y = (work_area.size.height as i32 - height - margin).max(0);
    let nudge_x = ((PILL_WINDOW_NUDGE_X as f64) * scale).round() as i32;
    let x = work_area.position.x + center_offset_x + nudge_x;
    let y = work_area.position.y + bottom_offset_y;
    let hwnd = window
        .hwnd()
        .map_err(|err| format!("failed to get floating pill HWND: {err}"))?;
    let succeeded = unsafe {
        SetWindowPos(
            hwnd.0 as windows_sys::Win32::Foundation::HWND,
            std::ptr::null_mut(),
            x,
            y,
            width,
            height,
            SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_NOZORDER | SWP_SHOWWINDOW,
        )
    };
    if succeeded == 0 {
        return Err("failed to resize and position floating pill without activation".to_string());
    }
    Ok(())
}

#[cfg(all(feature = "desktop", not(target_os = "windows")))]
fn resize_and_position_spike_window_no_activate(
    _app: &tauri::AppHandle,
    _window: &tauri::WebviewWindow,
    _width_logical: f64,
    _height_logical: f64,
    _bottom_margin_logical: f64,
) -> Result<(), String> {
    Err("pill Quick Controls spike is Windows-only".to_string())
}

#[cfg(feature = "desktop")]
fn show_pill_for_listening(app: &tauri::AppHandle) -> Result<(), String> {
    let pill = ensure_pill_window(app)?;

    // Showing is the final, non-negotiable operation. Failures in cosmetic
    // setup are logged but must never prevent a valid existing pill window
    // from becoming visible when dictation has been accepted.
    if let Err(err) = pill.set_ignore_cursor_events(true) {
        tracing::warn!("failed to make listening pill click-through: {err}");
    }
    if let Err(err) = pill.set_size(Size::Logical(LogicalSize::new(
        PILL_WINDOW_COMPACT_WIDTH,
        PILL_WINDOW_COMPACT_HEIGHT,
    ))) {
        tracing::warn!("failed to restore listening pill size: {err}");
    }
    if let Err(err) = position_pill_window(
        app,
        &pill,
        PILL_WINDOW_COMPACT_WIDTH,
        PILL_WINDOW_COMPACT_HEIGHT,
        PILL_WINDOW_COMPACT_BOTTOM_MARGIN,
    ) {
        tracing::warn!("failed to re-anchor listening pill: {err}");
    }
    pill.show()
        .map_err(|err| format!("failed to show listening pill: {err}"))
}

#[cfg(feature = "desktop")]
fn state_payload_hud_state(payload: &str) -> Option<state::VoiceWaveHudState> {
    serde_json::from_str::<serde_json::Value>(payload)
        .ok()
        .and_then(|value| {
            value
                .get("state")
                .cloned()
                .and_then(|state| serde_json::from_value(state).ok())
        })
}

#[cfg(all(test, feature = "desktop"))]
fn state_payload_is_listening(payload: &str) -> bool {
    state_payload_hud_state(payload)
        .is_some_and(|state| state == state::VoiceWaveHudState::Listening)
}

#[cfg(feature = "desktop")]
fn sync_pill_visibility(app: &tauri::AppHandle, visible: bool) -> Result<(), String> {
    app.state::<PillHitTargetState>()
        .desired_visible
        .store(visible, Ordering::Release);
    if visible {
        let spike = app.state::<PillQuickControlsSpikeState>();
        // Updating an unrelated setting must not resize an open tray card.
        // At every other time the original click-through pill owns the HUD.
        if !spike.enabled || spike.snapshot().invocation.is_none() {
            show_pill_for_listening(app)?;
            if spike.enabled {
                show_pill_hit_target(app)?;
            }
        }
    } else {
        hide_pill_hit_target(app);
        // A focused tray invocation temporarily owns the foreground. Restore
        // its original target before hiding the pill; leaving a hidden focused
        // WebView behind would make the next dictation insert into VoiceWave.
        dismiss_pill_quick_controls_spike_if_open(app)?;
        let pill = ensure_pill_window(app)?;
        pill.hide()
            .map_err(|err| format!("failed to hide floating pill: {err}"))?;
    }
    Ok(())
}

#[cfg(feature = "desktop")]
fn configure_main_window_close_behavior(app: &tauri::AppHandle, hide_to_tray: bool) {
    if let Some(main_window) = app.get_webview_window("main") {
        let window_for_handler = main_window.clone();
        main_window.on_window_event(move |event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if hide_to_tray {
                    api.prevent_close();
                    let _ = window_for_handler.hide();
                }
            }
        });
    }
}

/// Kill the worker side-processes before the app goes away. Windows does not
/// kill children with their parent, so without this a quit during the
/// first-run model download leaves a python.exe downloading ~487 MB with no UI
/// attached to it, and a speech worker would keep its model in (V)RAM.
/// Safe to call more than once (the helpers no-op when nothing is running).
#[cfg(feature = "desktop")]
fn shutdown_inference_workers() {
    kill_faster_whisper_workers();
    kill_asr_workers();
    kill_polish_worker();
}

#[cfg(feature = "desktop")]
fn configure_system_tray(app: &tauri::AppHandle) -> Result<(), String> {
    let show_item = MenuItemBuilder::with_id(TRAY_SHOW_ID, "Open VoiceWave")
        .build(app)
        .map_err(|err| format!("failed to build tray show item: {err}"))?;
    let quick_controls_item = app
        .try_state::<PillQuickControlsSpikeState>()
        .filter(|state| state.enabled)
        .map(|_| {
            MenuItemBuilder::with_id(TRAY_QUICK_CONTROLS_ID, "Quick Controls (experiment)")
                .build(app)
                .map_err(|err| format!("failed to build tray Quick Controls item: {err}"))
        })
        .transpose()?;
    let quit_item = MenuItemBuilder::with_id(TRAY_QUIT_ID, "Quit VoiceWave")
        .build(app)
        .map_err(|err| format!("failed to build tray quit item: {err}"))?;
    let menu_builder = MenuBuilder::new(app);
    let menu = if let Some(quick_controls_item) = quick_controls_item.as_ref() {
        menu_builder.items(&[&show_item, quick_controls_item, &quit_item])
    } else {
        menu_builder.items(&[&show_item, &quit_item])
    }
    .build()
    .map_err(|err| format!("failed to build tray menu: {err}"))?;

    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| "default window icon is unavailable for tray".to_string())?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("VoiceWave")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app: &tauri::AppHandle, event: tauri::menu::MenuEvent| {
            match event.id().as_ref() {
                TRAY_SHOW_ID => {
                    let app_handle = app.clone();
                    tauri::async_runtime::spawn(async move {
                        let _ = show_main_window(app_handle).await;
                    });
                }
                TRAY_QUICK_CONTROLS_ID => {
                    let spike = app.state::<PillQuickControlsSpikeState>();
                    if let Err(err) = apply_pill_quick_controls_spike_mode(
                        app,
                        &spike,
                        true,
                        PillQuickControlsInvocation::Focused,
                    ) {
                        tracing::warn!("failed to open tray Quick Controls: {err}");
                    }
                }
                TRAY_QUIT_ID => {
                    shutdown_inference_workers();
                    app.exit(0);
                }
                _ => {}
            }
        })
        .on_tray_icon_event(|tray: &tauri::tray::TrayIcon, event: TrayIconEvent| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle().clone();
                tauri::async_runtime::spawn(async move {
                    let _ = show_main_window(app).await;
                });
            }
        })
        .build(app)
        .map_err(|err| format!("failed to create system tray icon: {err}"))?;
    Ok(())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn get_voicewave_snapshot(
    runtime: State<'_, RuntimeContext>,
) -> Result<VoiceWaveSnapshot, String> {
    Ok(runtime.controller.snapshot().await)
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn load_settings(runtime: State<'_, RuntimeContext>) -> Result<VoiceWaveSettings, String> {
    Ok(runtime.controller.load_settings().await)
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn get_entitlement_snapshot(
    runtime: State<'_, RuntimeContext>,
) -> Result<EntitlementSnapshot, String> {
    runtime
        .controller
        .get_entitlement_snapshot()
        .await
        .map_err(|err| AppError::Controller(err).into())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn start_pro_checkout(
    runtime: State<'_, RuntimeContext>,
) -> Result<CheckoutLaunchResult, String> {
    runtime
        .controller
        .start_pro_checkout()
        .await
        .map_err(|err| AppError::Controller(err).into())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn refresh_entitlement(
    runtime: State<'_, RuntimeContext>,
) -> Result<EntitlementSnapshot, String> {
    runtime
        .controller
        .refresh_entitlement()
        .await
        .map_err(|err| AppError::Controller(err).into())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn restore_purchase(
    runtime: State<'_, RuntimeContext>,
) -> Result<EntitlementSnapshot, String> {
    runtime
        .controller
        .restore_purchase()
        .await
        .map_err(|err| AppError::Controller(err).into())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn open_billing_portal(
    runtime: State<'_, RuntimeContext>,
) -> Result<PortalLaunchResult, String> {
    runtime
        .controller
        .open_billing_portal()
        .await
        .map_err(|err| AppError::Controller(err).into())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn set_owner_device_override(
    runtime: State<'_, RuntimeContext>,
    enabled: bool,
    passphrase: String,
) -> Result<EntitlementSnapshot, String> {
    runtime
        .controller
        .set_owner_device_override(enabled, passphrase)
        .await
        .map_err(|err| AppError::Controller(err).into())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn update_settings(
    app: tauri::AppHandle,
    runtime: State<'_, RuntimeContext>,
    settings: VoiceWaveSettings,
) -> Result<VoiceWaveSettings, String> {
    let updated = runtime
        .controller
        .update_settings(settings)
        .await
        .map_err(|err| AppError::Controller(err).to_string())?;
    sync_pill_visibility(&app, updated.show_floating_hud)?;
    Ok(updated)
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn set_format_profile(
    runtime: State<'_, RuntimeContext>,
    profile: FormatProfile,
) -> Result<VoiceWaveSettings, String> {
    runtime
        .controller
        .set_format_profile(profile)
        .await
        .map_err(|err| AppError::Controller(err).into())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn set_active_domain_packs(
    runtime: State<'_, RuntimeContext>,
    packs: Vec<DomainPackId>,
) -> Result<VoiceWaveSettings, String> {
    runtime
        .controller
        .set_active_domain_packs(packs)
        .await
        .map_err(|err| AppError::Controller(err).into())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn set_app_profile_overrides(
    runtime: State<'_, RuntimeContext>,
    overrides: AppProfileOverrides,
) -> Result<VoiceWaveSettings, String> {
    runtime
        .controller
        .set_app_profile_overrides(overrides)
        .await
        .map_err(|err| AppError::Controller(err).into())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn set_code_mode_settings(
    runtime: State<'_, RuntimeContext>,
    settings: CodeModeSettings,
) -> Result<VoiceWaveSettings, String> {
    runtime
        .controller
        .set_code_mode_settings(settings)
        .await
        .map_err(|err| AppError::Controller(err).into())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn set_pro_post_processing_enabled(
    runtime: State<'_, RuntimeContext>,
    enabled: bool,
) -> Result<VoiceWaveSettings, String> {
    runtime
        .controller
        .set_pro_post_processing_enabled(enabled)
        .await
        .map_err(|err| AppError::Controller(err).into())
}

/// Select a polish profile (plan 010). Validates the wire string
/// ("standard" | "coding" | "writing" | "casual" | "literal"), applies the
/// profile's deterministic defaults (resetting advanced overrides), and
/// persists in ONE atomic settings write. Returns the updated settings,
/// including `polishProfile` and `polishProfileCustomized`.
#[cfg(feature = "desktop")]
#[tauri::command]
async fn set_dictation_profile(
    app: tauri::AppHandle,
    runtime: State<'_, RuntimeContext>,
    profile: String,
) -> Result<VoiceWaveSettings, String> {
    let updated = runtime
        .controller
        .set_dictation_profile(profile)
        .await
        .map_err(|err| String::from(AppError::Controller(err)))?;
    if let Err(err) = app.emit("voicewave://settings-changed", updated.clone()) {
        tracing::warn!("failed to emit settings change: {err}");
    }
    Ok(updated)
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn show_main_window(app: tauri::AppHandle) -> Result<(), String> {
    let Some(main_window) = app.get_webview_window("main") else {
        return Err("main window not available".to_string());
    };

    let _ = main_window.unminimize();
    main_window
        .show()
        .map_err(|err| format!("failed to show main window: {err}"))?;
    main_window
        .set_focus()
        .map_err(|err| format!("failed to focus main window: {err}"))?;
    Ok(())
}

/// Opens Windows Settings > Privacy > Microphone, where desktop-app mic
/// access is switched off when onboarding hears nothing. The URI is fixed, so
/// nothing from the frontend reaches the shell.
#[cfg(feature = "desktop")]
#[tauri::command]
fn open_microphone_privacy_settings() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer.exe")
            .arg("ms-settings:privacy-microphone")
            .spawn()
            .map(|_| ())
            .map_err(|err| format!("failed to open microphone privacy settings: {err}"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("microphone privacy settings are only available on Windows".to_string())
    }
}

/// What the "Report a problem" screen shows and pre-fills.
#[cfg(feature = "desktop")]
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SupportContext {
    app_version: String,
    os_version: String,
    log_dir: String,
}

#[cfg(feature = "desktop")]
#[tauri::command]
fn get_support_context() -> SupportContext {
    SupportContext {
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        os_version: support::os_version(),
        log_dir: support::log_dir()
            .map(|dir| dir.display().to_string())
            .unwrap_or_default(),
    }
}

/// Opens the prefilled GitHub bug form in the default browser. The frontend
/// only supplies the engine label; the URL itself is built from fixed parts.
#[cfg(feature = "desktop")]
#[tauri::command]
fn open_issue_report(engine: String) -> Result<(), String> {
    let url = support::issue_report_url(&engine)?;
    support::shell_open(url.as_ref())
        .map_err(|err| format!("failed to open the issue form: {err}"))
}

#[cfg(feature = "desktop")]
#[tauri::command]
fn open_logs_folder() -> Result<(), String> {
    let dir = support::log_dir().ok_or("cannot resolve the app data directory")?;
    std::fs::create_dir_all(&dir)
        .map_err(|err| format!("failed to create the logs folder: {err}"))?;
    support::shell_open(dir.as_os_str())
        .map_err(|err| format!("failed to open the logs folder: {err}"))
}

#[cfg(all(feature = "desktop", target_os = "windows"))]
fn capture_foreground_for_focused_spike(
    app: &tauri::AppHandle,
    pill: &tauri::WebviewWindow,
) -> Result<Option<isize>, String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    let foreground = unsafe { GetForegroundWindow() };
    if foreground.is_null() {
        return Ok(None);
    }
    let pill_hwnd = pill
        .hwnd()
        .map_err(|err| format!("failed to get floating pill HWND: {err}"))?
        .0 as windows_sys::Win32::Foundation::HWND;
    let main_hwnd = app
        .get_webview_window("main")
        .and_then(|main| main.hwnd().ok())
        .map(|hwnd| hwnd.0 as windows_sys::Win32::Foundation::HWND);
    if foreground == pill_hwnd || main_hwnd.is_some_and(|hwnd| hwnd == foreground) {
        return Ok(None);
    }
    Ok(Some(foreground as isize))
}

#[cfg(all(feature = "desktop", not(target_os = "windows")))]
fn capture_foreground_for_focused_spike(
    _app: &tauri::AppHandle,
    _pill: &tauri::WebviewWindow,
) -> Result<Option<isize>, String> {
    Ok(None)
}

#[cfg(all(feature = "desktop", target_os = "windows"))]
fn restore_focused_spike_target_if_safe(
    pill: &tauri::WebviewWindow,
    prior_hwnd: Option<isize>,
) -> Result<(), String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, IsWindow, SetForegroundWindow,
    };

    let Some(prior_hwnd) = prior_hwnd else {
        return Ok(());
    };
    let prior_hwnd = prior_hwnd as windows_sys::Win32::Foundation::HWND;
    let pill_hwnd = pill
        .hwnd()
        .map_err(|err| format!("failed to get floating pill HWND: {err}"))?
        .0 as windows_sys::Win32::Foundation::HWND;
    let foreground = unsafe { GetForegroundWindow() };
    // Never pull the user away from an application they intentionally focused
    // while the card was open. Restore only when the pill itself still owns
    // the foreground (focused tray path or an activating WebView2 control).
    if foreground != pill_hwnd {
        return Ok(());
    }
    if unsafe { IsWindow(prior_hwnd) } == 0 {
        return Err("previous foreground window is no longer valid".to_string());
    }
    if unsafe { SetForegroundWindow(prior_hwnd) } == 0 {
        return Err("Windows refused to restore the previous foreground window".to_string());
    }
    Ok(())
}

#[cfg(all(feature = "desktop", not(target_os = "windows")))]
fn restore_focused_spike_target_if_safe(
    _pill: &tauri::WebviewWindow,
    _prior_hwnd: Option<isize>,
) -> Result<(), String> {
    Ok(())
}

#[cfg(feature = "desktop")]
fn apply_pill_quick_controls_spike_mode(
    app: &tauri::AppHandle,
    state: &PillQuickControlsSpikeState,
    open: bool,
    invocation: PillQuickControlsInvocation,
) -> Result<(), String> {
    if !state.enabled {
        return Err(format!(
            "pill Quick Controls spike is disabled; set {PILL_QUICK_CONTROLS_SPIKE_ENV}=1 to enable it"
        ));
    }
    let pill = ensure_pill_window(app)?;

    if open {
        hide_pill_hit_target(app);
        // Capture for both paths. A native mouse click should not activate the
        // pill, but WebView2 control interaction still might; collapse can
        // then restore the original HWND without overriding a different app
        // the user intentionally focused.
        let prior_foreground_hwnd = capture_foreground_for_focused_spike(app, &pill)?;
        set_spike_window_no_activate(&pill, invocation == PillQuickControlsInvocation::Mouse)?;
        pill.set_ignore_cursor_events(false)
            .map_err(|err| format!("failed to make Quick Controls interactive: {err}"))?;
        resize_and_position_spike_window_no_activate(
            app,
            &pill,
            PILL_WINDOW_SPIKE_QUICK_WIDTH,
            PILL_WINDOW_SPIKE_QUICK_HEIGHT,
            PILL_WINDOW_SPIKE_QUICK_BOTTOM_MARGIN,
        )?;
        if invocation == PillQuickControlsInvocation::Focused {
            pill.set_focus()
                .map_err(|err| format!("failed to focus Quick Controls experiment: {err}"))?;
        }
        let mut inner = state
            .inner
            .lock()
            .map_err(|_| "pill Quick Controls spike state is poisoned".to_string())?;
        inner.session_generation = inner.session_generation.wrapping_add(1);
        inner.invocation = Some(invocation);
        inner.prior_foreground_hwnd = prior_foreground_hwnd;
    } else {
        let (active_invocation, prior_foreground_hwnd) = {
            let mut inner = state
                .inner
                .lock()
                .map_err(|_| "pill Quick Controls spike state is poisoned".to_string())?;
            inner.session_generation = inner.session_generation.wrapping_add(1);
            let snapshot = (inner.invocation.take(), inner.prior_foreground_hwnd.take());
            snapshot
        };
        // Always collapse back to the original 200x96, click-through visual
        // pill. The separate native hit target owns only the initial click.
        show_pill_for_listening(app)?;

        // The compact, non-activating geometry is already back in place
        // before we restore a tray invocation's target. If Windows rejects
        // the restore, the UI is still safely collapsed and cannot steal the
        // next dictation's insertion target.
        let restore_error = if active_invocation.is_some() {
            restore_focused_spike_target_if_safe(&pill, prior_foreground_hwnd).err()
        } else {
            None
        };

        let event = PillQuickControlsEvent {
            open: false,
            invocation: None,
        };
        app.emit_to(PILL_WINDOW_LABEL, "voicewave://pill-quick-controls", event)
            .map_err(|err| format!("failed to emit Quick Controls spike state: {err}"))?;
        if let Some(err) = restore_error {
            return Err(err);
        }
        return Ok(());
    }

    let event = PillQuickControlsEvent {
        open: true,
        invocation: Some(invocation),
    };
    app.emit_to(PILL_WINDOW_LABEL, "voicewave://pill-quick-controls", event)
        .map_err(|err| format!("failed to emit Quick Controls spike state: {err}"))?;
    Ok(())
}

#[cfg(feature = "desktop")]
fn dismiss_pill_quick_controls_spike_if_open(app: &tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<PillQuickControlsSpikeState>();
    let invocation = state
        .inner
        .lock()
        .map_err(|_| "pill Quick Controls spike state is poisoned".to_string())?
        .invocation;
    if let Some(invocation) = invocation {
        apply_pill_quick_controls_spike_mode(app, &state, false, invocation)?;
    }
    Ok(())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn get_pill_quick_controls_snapshot(
    spike: State<'_, PillQuickControlsSpikeState>,
) -> Result<PillQuickControlsSnapshot, String> {
    Ok(spike.snapshot())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn set_pill_quick_controls_mode(
    app: tauri::AppHandle,
    spike: State<'_, PillQuickControlsSpikeState>,
    open: bool,
) -> Result<(), String> {
    if open {
        return Err(
            "direct pill click is disabled after the Windows interaction gate failed".to_string(),
        );
    }
    let invocation = spike
        .inner
        .lock()
        .map_err(|_| "pill Quick Controls spike state is poisoned".to_string())?
        .invocation
        .unwrap_or(PillQuickControlsInvocation::Focused);
    apply_pill_quick_controls_spike_mode(&app, &spike, false, invocation)?;
    show_pill_hit_target(&app)
}

#[cfg(feature = "desktop")]
fn apply_pill_window_mode(
    app: &tauri::AppHandle,
    mode: PillWindowMode,
    interactive: bool,
) -> Result<(), String> {
    if app
        .state::<PillQuickControlsSpikeState>()
        .snapshot()
        .invocation
        .is_some()
    {
        return Ok(());
    }

    let pill = ensure_pill_window(app)?;
    let review_mode = mode == PillWindowMode::Review;
    let notice_mode = mode == PillWindowMode::Notice;
    set_pill_hit_target_blocker(app, Some(review_mode), Some(notice_mode), None)?;

    if mode != PillWindowMode::Compact {
        hide_pill_hit_target(app);
    }

    let pointer_interactive = review_mode || (notice_mode && interactive);
    let _ = pill.set_ignore_cursor_events(!pointer_interactive);
    let (width, height, margin) = pill_window_geometry(mode);
    pill.set_size(Size::Logical(LogicalSize::new(width, height)))
        .map_err(|err| format!("failed to resize floating pill for {mode:?}: {err}"))?;
    position_pill_window(app, &pill, width, height, margin)?;

    if notice_mode {
        // Notices may be emitted while the status HUD is between visible
        // lifecycle states; the user still needs to see the notice.
        let _ = pill.show();
    } else if mode == PillWindowMode::Compact {
        show_pill_hit_target(app)?;
    }
    Ok(())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn set_pill_window_mode(
    app: tauri::AppHandle,
    mode: PillWindowMode,
    interactive: Option<bool>,
) -> Result<(), String> {
    apply_pill_window_mode(&app, mode, interactive.unwrap_or(false))
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn set_pill_review_mode(app: tauri::AppHandle, review_mode: bool) -> Result<(), String> {
    let mode = if review_mode {
        PillWindowMode::Review
    } else {
        PillWindowMode::Compact
    };
    apply_pill_window_mode(&app, mode, review_mode)
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn set_pill_notice_mode(
    app: tauri::AppHandle,
    notice_mode: bool,
    interactive: Option<bool>,
) -> Result<(), String> {
    let mode = if notice_mode {
        PillWindowMode::Notice
    } else {
        PillWindowMode::Compact
    };
    apply_pill_window_mode(&app, mode, interactive.unwrap_or(false))
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn copy_text_to_clipboard(text: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        arboard::Clipboard::new()
            .and_then(|mut clipboard| clipboard.set_text(text))
            .map_err(|err| format!("clipboard copy failed: {err}"))
    })
    .await
    .map_err(|err| format!("clipboard task failed: {err}"))?
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn get_diagnostics_status(
    runtime: State<'_, RuntimeContext>,
) -> Result<DiagnosticsStatus, String> {
    Ok(runtime.controller.get_diagnostics_status().await)
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn set_diagnostics_opt_in(
    runtime: State<'_, RuntimeContext>,
    enabled: bool,
) -> Result<DiagnosticsStatus, String> {
    runtime
        .controller
        .set_diagnostics_opt_in(enabled)
        .await
        .map_err(|err| AppError::Controller(err).into())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn export_diagnostics_bundle(
    runtime: State<'_, RuntimeContext>,
) -> Result<DiagnosticsExportResult, String> {
    runtime
        .controller
        .export_diagnostics_bundle()
        .await
        .map_err(|err| AppError::Controller(err).into())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn start_dictation(
    app: tauri::AppHandle,
    runtime: State<'_, RuntimeContext>,
    mode: Option<DictationMode>,
    control_mode: Option<DictationControlMode>,
) -> Result<(), String> {
    runtime
        .controller
        .start_dictation(
            app,
            mode.unwrap_or_default(),
            control_mode.unwrap_or_default(),
        )
        .await
        .map_err(|err| AppError::Controller(err).into())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn cancel_dictation(
    app: tauri::AppHandle,
    runtime: State<'_, RuntimeContext>,
) -> Result<(), String> {
    runtime.controller.cancel_dictation(app).await;
    Ok(())
}

/// Pill "Try again" after a failed transcription (plan 020).
#[cfg(feature = "desktop")]
#[tauri::command]
async fn retry_last_dictation(
    app: tauri::AppHandle,
    runtime: State<'_, RuntimeContext>,
) -> Result<(), String> {
    runtime
        .controller
        .retry_last_dictation(app)
        .await
        .map_err(|err| AppError::Controller(err).into())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn stop_dictation(
    app: tauri::AppHandle,
    runtime: State<'_, RuntimeContext>,
) -> Result<(), String> {
    runtime.controller.stop_dictation(app).await;
    Ok(())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn load_hotkey_config(runtime: State<'_, RuntimeContext>) -> Result<HotkeySnapshot, String> {
    Ok(runtime.controller.hotkey_snapshot().await)
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn update_hotkey_config(
    runtime: State<'_, RuntimeContext>,
    config: HotkeyConfig,
) -> Result<HotkeySnapshot, String> {
    runtime
        .controller
        .update_hotkey_config(config)
        .await
        .map_err(|err| AppError::Controller(err).into())
}

/// Pauses the global hotkey while Settings records a new shortcut.
#[cfg(feature = "desktop")]
#[tauri::command]
async fn set_hotkey_capture(runtime: State<'_, RuntimeContext>, active: bool) -> Result<(), String> {
    runtime.controller.set_hotkey_capture(active).await;
    Ok(())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn get_permission_snapshot(
    runtime: State<'_, RuntimeContext>,
) -> Result<PermissionSnapshot, String> {
    Ok(runtime.controller.permission_snapshot().await)
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn list_input_devices(runtime: State<'_, RuntimeContext>) -> Result<Vec<String>, String> {
    Ok(runtime.controller.list_input_devices().await)
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn request_microphone_access(
    app: tauri::AppHandle,
    runtime: State<'_, RuntimeContext>,
) -> Result<PermissionSnapshot, String> {
    Ok(runtime.controller.request_microphone_access(app).await)
}

/// Forward live mic-level frames to the main window while the onboarding mic
/// check is on screen. Steady state keeps them pill-only (PERF-05).
#[cfg(feature = "desktop")]
#[tauri::command]
async fn set_mic_level_forwarding(enabled: bool) -> Result<(), String> {
    crate::state::set_mic_level_forwarding(enabled);
    Ok(())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn start_mic_level_monitor(
    app: tauri::AppHandle,
    runtime: State<'_, RuntimeContext>,
) -> Result<(), String> {
    runtime
        .controller
        .start_mic_level_monitor(app)
        .await
        .map_err(|err| AppError::Controller(err).into())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn stop_mic_level_monitor(runtime: State<'_, RuntimeContext>) -> Result<(), String> {
    runtime.controller.stop_mic_level_monitor().await;
    Ok(())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn run_audio_quality_diagnostic(
    app: tauri::AppHandle,
    runtime: State<'_, RuntimeContext>,
    duration_ms: Option<u64>,
) -> Result<AudioQualityReport, String> {
    runtime
        .controller
        .run_audio_quality_diagnostic(app, duration_ms)
        .await
        .map_err(|err| AppError::Controller(err).into())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn insert_text(
    app: tauri::AppHandle,
    runtime: State<'_, RuntimeContext>,
    payload: InsertTextRequest,
) -> Result<InsertResult, String> {
    runtime
        .controller
        .insert_text(app, payload)
        .await
        .map_err(|err| AppError::Controller(err).into())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn undo_last_insertion(
    app: tauri::AppHandle,
    runtime: State<'_, RuntimeContext>,
) -> Result<UndoResult, String> {
    Ok(runtime.controller.undo_last_insertion(app).await)
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn get_recent_insertions(
    runtime: State<'_, RuntimeContext>,
    limit: Option<usize>,
) -> Result<Vec<RecentInsertion>, String> {
    Ok(runtime.controller.recent_insertions(limit).await)
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn trigger_hotkey_action(
    app: tauri::AppHandle,
    runtime: State<'_, RuntimeContext>,
    action: HotkeyAction,
    phase: HotkeyPhase,
) -> Result<(), String> {
    runtime
        .controller
        .trigger_hotkey_action(app, action, phase)
        .await
        .map_err(|err| AppError::Controller(err).into())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn list_model_catalog(
    runtime: State<'_, RuntimeContext>,
) -> Result<Vec<ModelCatalogItem>, String> {
    Ok(runtime.controller.list_model_catalog().await)
}

/// Plan 019 engine state for the UI: CPU support, the chosen GPU, where the
/// active model runs, the one-time GPU check and the upgrade offer.
#[cfg(feature = "desktop")]
#[tauri::command]
async fn get_speech_engine_status(
    runtime: State<'_, RuntimeContext>,
) -> Result<state::SpeechEngineStatus, String> {
    Ok(runtime.controller.speech_engine_status().await)
}

/// Answer the one-time GPU upgrade notice; returns the model to download
/// when accepted.
#[cfg(feature = "desktop")]
#[tauri::command]
async fn answer_gpu_upgrade(
    runtime: State<'_, RuntimeContext>,
    accept: bool,
) -> Result<Option<String>, String> {
    runtime
        .controller
        .answer_gpu_upgrade(accept)
        .await
        .map_err(|err| AppError::Controller(err).into())
}

/// Qualify the accepted upgrade model and switch to it (no-op otherwise).
/// Can take a minute on first GPU use; the UI polls the engine status.
#[cfg(feature = "desktop")]
#[tauri::command]
async fn finish_gpu_upgrade(
    app: tauri::AppHandle,
    runtime: State<'_, RuntimeContext>,
) -> Result<(), String> {
    runtime
        .controller
        .finish_gpu_upgrade(app)
        .await
        .map_err(|err| AppError::Controller(err).into())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn remove_model(
    app: tauri::AppHandle,
    runtime: State<'_, RuntimeContext>,
    model_id: String,
) -> Result<Vec<InstalledModel>, String> {
    runtime
        .controller
        .remove_model(app, model_id)
        .await
        .map_err(|err| AppError::Controller(err).into())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn list_installed_models(
    runtime: State<'_, RuntimeContext>,
) -> Result<Vec<InstalledModel>, String> {
    Ok(runtime.controller.list_installed_models().await)
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn get_model_status(
    runtime: State<'_, RuntimeContext>,
    model_id: String,
) -> Result<ModelStatus, String> {
    runtime
        .controller
        .get_model_status(model_id)
        .await
        .map_err(|err| AppError::Controller(err).into())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn download_model(
    app: tauri::AppHandle,
    runtime: State<'_, RuntimeContext>,
    request: ModelDownloadRequest,
) -> Result<ModelStatus, String> {
    runtime
        .controller
        .download_model(app, request)
        .await
        .map_err(|err| AppError::Controller(err).into())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn cancel_model_download(
    app: tauri::AppHandle,
    runtime: State<'_, RuntimeContext>,
    model_id: String,
) -> Result<ModelStatus, String> {
    runtime
        .controller
        .cancel_model_download(app, model_id)
        .await
        .map_err(|err| AppError::Controller(err).into())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn pause_model_download(
    app: tauri::AppHandle,
    runtime: State<'_, RuntimeContext>,
    model_id: String,
) -> Result<ModelStatus, String> {
    runtime
        .controller
        .pause_model_download(app, model_id)
        .await
        .map_err(|err| AppError::Controller(err).into())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn resume_model_download(
    app: tauri::AppHandle,
    runtime: State<'_, RuntimeContext>,
    model_id: String,
) -> Result<ModelStatus, String> {
    runtime
        .controller
        .resume_model_download(app, model_id)
        .await
        .map_err(|err| AppError::Controller(err).into())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn set_active_model(
    app: tauri::AppHandle,
    runtime: State<'_, RuntimeContext>,
    model_id: String,
) -> Result<VoiceWaveSettings, String> {
    runtime
        .controller
        .set_active_model(app, model_id)
        .await
        .map_err(|err| AppError::Controller(err).into())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn run_model_benchmark(
    app: tauri::AppHandle,
    runtime: State<'_, RuntimeContext>,
    request: Option<BenchmarkRequest>,
) -> Result<BenchmarkRun, String> {
    runtime
        .controller
        .run_model_benchmark(
            app,
            request.unwrap_or(BenchmarkRequest {
                model_ids: None,
                runs_per_model: None,
                partial_delay_ms: None,
            }),
        )
        .await
        .map_err(|err| AppError::Controller(err).into())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn get_benchmark_results(
    runtime: State<'_, RuntimeContext>,
) -> Result<Option<BenchmarkRun>, String> {
    Ok(runtime.controller.get_benchmark_results().await)
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn recommend_model(
    runtime: State<'_, RuntimeContext>,
    constraints: Option<RecommendationConstraints>,
) -> Result<ModelRecommendation, String> {
    runtime
        .controller
        .recommend_model(constraints.unwrap_or_default())
        .await
        .map_err(|err| AppError::Controller(err).into())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn get_session_history(
    runtime: State<'_, RuntimeContext>,
    query: Option<SessionHistoryQuery>,
) -> Result<Vec<SessionHistoryRecord>, String> {
    Ok(runtime
        .controller
        .get_session_history(query.unwrap_or_default())
        .await)
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn search_session_history(
    runtime: State<'_, RuntimeContext>,
    query: String,
    tags: Option<Vec<String>>,
    starred: Option<bool>,
) -> Result<Vec<SessionHistoryRecord>, String> {
    runtime
        .controller
        .search_session_history(query, tags, starred)
        .await
        .map_err(|err| AppError::Controller(err).into())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn tag_session(
    runtime: State<'_, RuntimeContext>,
    record_id: String,
    tag: String,
) -> Result<SessionHistoryRecord, String> {
    runtime
        .controller
        .tag_session(record_id, tag)
        .await
        .map_err(|err| AppError::Controller(err).into())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn toggle_star_session(
    runtime: State<'_, RuntimeContext>,
    record_id: String,
    starred: bool,
) -> Result<SessionHistoryRecord, String> {
    runtime
        .controller
        .toggle_star_session(record_id, starred)
        .await
        .map_err(|err| AppError::Controller(err).into())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn export_session_history_preset(
    runtime: State<'_, RuntimeContext>,
    preset: HistoryExportPreset,
) -> Result<HistoryExportResult, String> {
    runtime
        .controller
        .export_session_history_preset(preset)
        .await
        .map_err(|err| AppError::Controller(err).into())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn set_history_retention(
    app: tauri::AppHandle,
    runtime: State<'_, RuntimeContext>,
    policy: RetentionPolicy,
) -> Result<RetentionPolicy, String> {
    runtime
        .controller
        .set_history_retention(app, policy)
        .await
        .map_err(|err| AppError::Controller(err).into())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn get_history_retention(
    runtime: State<'_, RuntimeContext>,
) -> Result<RetentionPolicy, String> {
    Ok(runtime.controller.get_history_retention().await)
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn get_stats_summary(
    runtime: State<'_, RuntimeContext>,
    range_days: Option<u32>,
) -> Result<StatsSummary, String> {
    // Normalize to a supported window (30/91/365, default 30).
    let window = match range_days {
        Some(d) if d == 30 || d == 91 || d == 365 => d,
        _ => 30,
    };
    Ok(runtime.controller.get_stats_summary(window).await)
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn prune_history_now(
    app: tauri::AppHandle,
    runtime: State<'_, RuntimeContext>,
) -> Result<usize, String> {
    runtime
        .controller
        .prune_history_now(app)
        .await
        .map_err(|err| AppError::Controller(err).into())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn clear_history(
    app: tauri::AppHandle,
    runtime: State<'_, RuntimeContext>,
) -> Result<usize, String> {
    runtime
        .controller
        .clear_history(app)
        .await
        .map_err(|err| AppError::Controller(err).into())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn get_dictionary_queue(
    runtime: State<'_, RuntimeContext>,
    limit: Option<usize>,
) -> Result<Vec<DictionaryQueueItem>, String> {
    Ok(runtime.controller.get_dictionary_queue(limit).await)
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn approve_dictionary_entry(
    app: tauri::AppHandle,
    runtime: State<'_, RuntimeContext>,
    entry_id: String,
    normalized_text: Option<String>,
) -> Result<DictionaryTerm, String> {
    runtime
        .controller
        .approve_dictionary_entry(app, entry_id, normalized_text)
        .await
        .map_err(|err| AppError::Controller(err).into())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn reject_dictionary_entry(
    app: tauri::AppHandle,
    runtime: State<'_, RuntimeContext>,
    entry_id: String,
    reason: Option<String>,
) -> Result<(), String> {
    runtime
        .controller
        .reject_dictionary_entry(app, entry_id, reason)
        .await
        .map_err(|err| AppError::Controller(err).into())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn get_dictionary_terms(
    runtime: State<'_, RuntimeContext>,
    query: Option<String>,
) -> Result<Vec<DictionaryTerm>, String> {
    Ok(runtime.controller.get_dictionary_terms(query).await)
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn remove_dictionary_term(
    app: tauri::AppHandle,
    runtime: State<'_, RuntimeContext>,
    term_id: String,
) -> Result<(), String> {
    runtime
        .controller
        .remove_dictionary_term(app, term_id)
        .await
        .map_err(|err| AppError::Controller(err).into())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn add_dictionary_term(
    app: tauri::AppHandle,
    runtime: State<'_, RuntimeContext>,
    term: String,
) -> Result<DictionaryTerm, String> {
    runtime
        .controller
        .add_dictionary_term(app, term)
        .await
        .map_err(|err| AppError::Controller(err).into())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn export_dictionary(runtime: State<'_, RuntimeContext>) -> Result<DictionaryExport, String> {
    Ok(runtime.controller.export_dictionary().await)
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn import_dictionary(
    app: tauri::AppHandle,
    runtime: State<'_, RuntimeContext>,
    payload: String,
) -> Result<DictionaryImportSummary, String> {
    runtime
        .controller
        .import_dictionary(app, payload)
        .await
        .map_err(|err| AppError::Controller(err).into())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn get_dictionary_sync_records(
    runtime: State<'_, RuntimeContext>,
) -> Result<Vec<DictionarySyncRecord>, String> {
    Ok(runtime.controller.get_dictionary_sync_records().await)
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn reconcile_dictionary_records(
    runtime: State<'_, RuntimeContext>,
    records: Vec<DictionarySyncRecord>,
) -> Result<DictionaryReconcileResult, String> {
    runtime
        .controller
        .reconcile_dictionary_records(records)
        .await
        .map_err(|err| AppError::Controller(err).into())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn list_voice_snippets(
    runtime: State<'_, RuntimeContext>,
    query: Option<String>,
) -> Result<Vec<VoiceSnippet>, SnippetCommandError> {
    Ok(runtime.controller.list_voice_snippets(query).await)
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn add_voice_snippet(
    runtime: State<'_, RuntimeContext>,
    trigger: String,
    expansion: String,
) -> Result<VoiceSnippet, SnippetCommandError> {
    runtime
        .controller
        .add_voice_snippet(trigger, expansion)
        .await
        .map_err(Into::into)
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn update_voice_snippet(
    runtime: State<'_, RuntimeContext>,
    snippet_id: String,
    trigger: String,
    expansion: String,
) -> Result<VoiceSnippet, SnippetCommandError> {
    runtime
        .controller
        .update_voice_snippet(snippet_id, trigger, expansion)
        .await
        .map_err(Into::into)
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn remove_voice_snippet(
    runtime: State<'_, RuntimeContext>,
    snippet_id: String,
) -> Result<(), SnippetCommandError> {
    runtime
        .controller
        .remove_voice_snippet(snippet_id)
        .await
        .map_err(Into::into)
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn get_voice_snippet_sync_records(
    runtime: State<'_, RuntimeContext>,
) -> Result<Vec<VoiceSnippetSyncRecord>, SnippetCommandError> {
    Ok(runtime.controller.get_voice_snippet_sync_records().await)
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn reconcile_voice_snippet_records(
    runtime: State<'_, RuntimeContext>,
    records: Vec<VoiceSnippetSyncRecord>,
) -> Result<VoiceSnippetReconcileResult, SnippetCommandError> {
    runtime
        .controller
        .reconcile_voice_snippet_records(records)
        .await
        .map_err(Into::into)
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PolishModelStatus {
    present: bool,
    downloading: bool,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PolishModelProgress {
    downloaded: u64,
    total: u64,
    done: bool,
    error: Option<String>,
}

/// Whether the on-device AI-polish model is downloaded and/or downloading.
/// The settings UI polls this to decide whether enabling the toggle should
/// kick off a download.
#[cfg(feature = "desktop")]
#[tauri::command]
async fn polish_model_status() -> Result<PolishModelStatus, String> {
    Ok(PolishModelStatus {
        present: crate::inference::llm_polish::is_polish_model_present(),
        downloading: crate::inference::llm_polish::is_polish_model_downloading(),
    })
}

/// Download the ~1 GB AI-polish model to the app data dir, emitting
/// `voicewave://polish-model-progress` events so the settings UI can show a
/// progress bar. Idempotent and single-flighted; safe to call on every enable.
#[cfg(feature = "desktop")]
#[tauri::command]
async fn download_polish_model(
    app: tauri::AppHandle,
    runtime: State<'_, RuntimeContext>,
) -> Result<(), String> {
    if !crate::inference::llm_polish::polish_supported() {
        return Err(crate::inference::llm_polish::POLISH_UNSUPPORTED_MESSAGE.to_string());
    }
    let progress_app = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        crate::inference::llm_polish::download_polish_model(|downloaded, total| {
            let _ = progress_app.emit(
                "voicewave://polish-model-progress",
                PolishModelProgress {
                    downloaded,
                    total,
                    done: false,
                    error: None,
                },
            );
        })
    })
    .await
    .map_err(|err| format!("polish model download task failed: {err}"))?;

    match result {
        Ok(_) => {
            // Keep the UI in its preparing state until the selected profile's
            // prompt has been prefetched as well as the GGUF downloaded.
            runtime.controller.prewarm_active_polish_profile().await;
            let _ = app.emit(
                "voicewave://polish-model-progress",
                PolishModelProgress {
                    downloaded: 0,
                    total: 0,
                    done: true,
                    error: None,
                },
            );
            Ok(())
        }
        Err(err) => {
            let _ = app.emit(
                "voicewave://polish-model-progress",
                PolishModelProgress {
                    downloaded: 0,
                    total: 0,
                    done: false,
                    error: Some(err.clone()),
                },
            );
            Err(err)
        }
    }
}

#[cfg(feature = "desktop")]
pub fn run() {
    // First, so every later line (and any panic) reaches the log file. This
    // runs after main()'s single-instance claim, so a second launch that
    // exits there never opens or rotates the log.
    support::init();

    tauri::Builder::default()
        // Single instance: two event-driven keyboard hooks would BOTH capture
        // and insert every dictation (text lands twice). A second launch must
        // focus the existing window instead. Must be the first plugin
        // registered.
        // The PRIMARY guard now lives in `main()` (a named mutex claimed
        // before any Tauri code); this plugin is the second line of defence
        // and only ever runs after that guard has let us through.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        // Auto-update: checks the GitHub `latest.json` endpoint configured in
        // tauri.conf.json, verifies the artifact signature against `pubkey`,
        // and (driven from the frontend) downloads + installs the new NSIS
        // setup. `process` provides the relaunch after a successful install.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        // "Start with Windows": the Run-key entry launches us with
        // `--autostart`, which keeps the window hidden (see `setup`).
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![AUTOSTART_ARG]),
        ))
        .setup(|app| {
            // The main window is created hidden (tauri.conf.json) so a login
            // launch never flashes it. Normal launches show it straight away.
            let launched_at_login = std::env::args().any(|arg| arg == AUTOSTART_ARG);
            if !launched_at_login {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                }
            }
            let controller = Arc::new(
                VoiceWaveController::new()
                    .map_err(|err| -> Box<dyn std::error::Error> { Box::new(err) })?,
            );
            let initial_settings = tauri::async_runtime::block_on(controller.load_settings());

            let controller_for_hotkeys = controller.clone();
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                controller_for_hotkeys
                    .ensure_hotkey_runtime_monitor(app_handle)
                    .await;
            });

            // Cold-start fix: pre-load the active model's weights into its
            // runtime cache in the background. Without this, the first
            // dictation after app launch costs ~2-5 s of model deserialization
            // + CUDA context init + kernel compilation. After warmup it drops
            // to ~500 ms. Fire-and-forget; never blocks startup.
            let controller_for_prewarm = controller.clone();
            let app_for_upgrade = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                controller_for_prewarm.prewarm_active_model().await;
                // A GPU upgrade accepted before a restart: qualify + switch.
                if let Err(err) = controller_for_prewarm.finish_gpu_upgrade(app_for_upgrade).await {
                    tracing::warn!("resuming the GPU upgrade failed: {err}");
                }
                // Warm the optional formatter only after ASR is ready. Running
                // both model preloads concurrently causes avoidable CPU, disk,
                // and memory contention on budget machines.
                controller_for_prewarm.prewarm_active_polish_profile().await;
            });

            let controller_for_pill_state = controller.clone();
            app.manage(RuntimeContext { controller });
            app.manage(PillQuickControlsSpikeState::from_env());
            app.manage(PillHitTargetState::default());
            initialize_pill_hit_target(&app.handle().clone(), false).map_err(
                |err| -> Box<dyn std::error::Error> { Box::new(std::io::Error::other(err)) },
            )?;

            let app_handle = app.handle().clone();
            let tray_ready = match configure_system_tray(&app_handle) {
                Ok(_) => true,
                Err(err) => {
                    tracing::warn!("tray setup failed, falling back to close-to-exit: {err}");
                    false
                }
            };
            configure_main_window_close_behavior(&app_handle, tray_ready);
            // A login launch lives in the tray; without a tray there would be
            // no way back to the app, so show the window instead.
            if launched_at_login && !tray_ready {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                }
            }
            sync_pill_visibility(&app.handle().clone(), initial_settings.show_floating_hud)
                .map_err(|err| -> Box<dyn std::error::Error> {
                    Box::new(std::io::Error::other(err))
                })?;
            // Stores recover silently rather than aborting the launch; this is
            // the one place that can still tell the user it happened.
            state::announce_store_resets(&app.handle().clone());
            // Open the cue output stream now so the first hotkey press
            // doesn't pay the audio-device-open latency.
            tauri::async_runtime::spawn_blocking(cue::prewarm);
            // Reassert the complete visible-pill invariant at the start of
            // every accepted dictation. The window can have been recreated
            // hidden after a WebView failure, collapsed from review mode, or
            // left on a stale monitor; positioning alone was not enough.
            {
                let listening_handle = app.handle().clone();
                app.handle().listen("voicewave://state", move |event| {
                    let Some(hud_state) = state_payload_hud_state(event.payload()) else {
                        return;
                    };
                    let handle = listening_handle.clone();
                    let lifecycle_blocked = hud_state != state::VoiceWaveHudState::Idle;
                    let _ =
                        set_pill_hit_target_blocker(&handle, None, None, Some(lifecycle_blocked));
                    let controller = controller_for_pill_state.clone();
                    tauri::async_runtime::spawn(async move {
                        // Preserve the explicit user preference to disable
                        // the HUD. Recording/transcription must never leave a
                        // click target over the user's foreground app.
                        if !controller.load_settings().await.show_floating_hud {
                            hide_pill_hit_target(&handle);
                            return;
                        }
                        match hud_state {
                            state::VoiceWaveHudState::Listening => {
                                hide_pill_hit_target(&handle);
                                let result = dismiss_pill_quick_controls_spike_if_open(&handle)
                                    .and_then(|_| show_pill_for_listening(&handle));
                                if let Err(err) = result {
                                    tracing::warn!("listening pill recovery failed: {err}");
                                }
                            }
                            state::VoiceWaveHudState::Transcribing => {
                                hide_pill_hit_target(&handle);
                            }
                            state::VoiceWaveHudState::Idle => {
                                let _ = show_pill_hit_target(&handle);
                            }
                            state::VoiceWaveHudState::Inserted
                            | state::VoiceWaveHudState::Error => {
                                hide_pill_hit_target(&handle);
                                tokio::time::sleep(std::time::Duration::from_millis(900)).await;
                                let snapshot = controller.snapshot().await;
                                if matches!(
                                    snapshot.state,
                                    state::VoiceWaveHudState::Inserted
                                        | state::VoiceWaveHudState::Error
                                ) {
                                    let _ = set_pill_hit_target_blocker(
                                        &handle,
                                        None,
                                        None,
                                        Some(false),
                                    );
                                    let _ = show_pill_hit_target(&handle);
                                }
                            }
                        }
                    });
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_voicewave_snapshot,
            load_settings,
            get_entitlement_snapshot,
            start_pro_checkout,
            refresh_entitlement,
            restore_purchase,
            open_billing_portal,
            set_owner_device_override,
            update_settings,
            set_format_profile,
            set_active_domain_packs,
            set_app_profile_overrides,
            set_code_mode_settings,
            set_pro_post_processing_enabled,
            set_dictation_profile,
            show_main_window,
            open_microphone_privacy_settings,
            get_support_context,
            open_issue_report,
            open_logs_folder,
            get_pill_quick_controls_snapshot,
            set_pill_quick_controls_mode,
            set_pill_window_mode,
            set_pill_review_mode,
            set_pill_notice_mode,
            copy_text_to_clipboard,
            get_diagnostics_status,
            set_diagnostics_opt_in,
            export_diagnostics_bundle,
            start_dictation,
            cancel_dictation,
            retry_last_dictation,
            stop_dictation,
            load_hotkey_config,
            update_hotkey_config,
            set_hotkey_capture,
            get_permission_snapshot,
            list_input_devices,
            request_microphone_access,
            start_mic_level_monitor,
            stop_mic_level_monitor,
            set_mic_level_forwarding,
            run_audio_quality_diagnostic,
            insert_text,
            undo_last_insertion,
            get_recent_insertions,
            trigger_hotkey_action,
            list_model_catalog,
            get_speech_engine_status,
            answer_gpu_upgrade,
            finish_gpu_upgrade,
            remove_model,
            list_installed_models,
            get_model_status,
            download_model,
            cancel_model_download,
            pause_model_download,
            resume_model_download,
            set_active_model,
            run_model_benchmark,
            get_benchmark_results,
            recommend_model,
            get_session_history,
            search_session_history,
            tag_session,
            toggle_star_session,
            export_session_history_preset,
            set_history_retention,
            get_history_retention,
            get_stats_summary,
            prune_history_now,
            clear_history,
            get_dictionary_queue,
            approve_dictionary_entry,
            reject_dictionary_entry,
            get_dictionary_terms,
            remove_dictionary_term,
            add_dictionary_term,
            export_dictionary,
            import_dictionary,
            get_dictionary_sync_records,
            reconcile_dictionary_records,
            list_voice_snippets,
            add_voice_snippet,
            update_voice_snippet,
            remove_voice_snippet,
            get_voice_snippet_sync_records,
            reconcile_voice_snippet_records,
            polish_model_status,
            download_polish_model
        ])
        .build(tauri::generate_context!())
        .expect("error while running voicewave tauri app")
        .run(|_app, event| {
            // Covers every exit route (window close, app.exit, OS shutdown),
            // not just the tray Quit item.
            if matches!(event, tauri::RunEvent::Exit) {
                shutdown_inference_workers();
            }
        });
}

#[cfg(not(feature = "desktop"))]
pub fn run() {
    panic!("desktop runtime requested without the 'desktop' feature enabled")
}

#[cfg(all(test, feature = "desktop"))]
mod desktop_tests {
    use super::{
        env_flag_enabled, fullscreen_geometry_matches, pill_window_geometry,
        state_payload_is_listening, PillQuickControlsInvocation, PillQuickControlsSpikeInner,
        PillQuickControlsSpikeState, PillWindowMode, PILL_WINDOW_COMPACT_BOTTOM_MARGIN,
        PILL_WINDOW_COMPACT_HEIGHT, PILL_WINDOW_COMPACT_WIDTH, PILL_WINDOW_NOTICE_BOTTOM_MARGIN,
        PILL_WINDOW_NOTICE_HEIGHT, PILL_WINDOW_NOTICE_WIDTH, PILL_WINDOW_REVIEW_BOTTOM_MARGIN,
        PILL_WINDOW_REVIEW_HEIGHT, PILL_WINDOW_REVIEW_WIDTH,
    };
    use std::sync::Mutex;

    #[test]
    fn pill_recovery_only_runs_for_exact_listening_state() {
        assert!(state_payload_is_listening(
            r#"{"state":"listening","message":"Listening for speech..."}"#
        ));
        assert!(!state_payload_is_listening(
            r#"{"state":"processing","message":"listening back"}"#
        ));
        assert!(!state_payload_is_listening("not json"));
    }

    #[test]
    fn quick_controls_spike_flag_is_explicit_and_case_insensitive() {
        for enabled in ["1", "true", "TRUE", " yes ", "On"] {
            assert!(
                env_flag_enabled(Some(enabled)),
                "{enabled} should enable spike"
            );
        }
        for disabled in ["", "0", "false", "enabled", "quick-controls"] {
            assert!(
                !env_flag_enabled(Some(disabled)),
                "{disabled} should leave spike off"
            );
        }
        assert!(!env_flag_enabled(None));
    }

    #[test]
    fn quick_controls_snapshot_exposes_only_enabled_and_invocation() {
        let state = PillQuickControlsSpikeState {
            enabled: true,
            inner: Mutex::new(PillQuickControlsSpikeInner {
                invocation: Some(PillQuickControlsInvocation::Focused),
                session_generation: 1,
                prior_foreground_hwnd: Some(42),
            }),
        };

        let snapshot = state.snapshot();
        assert!(snapshot.enabled);
        assert_eq!(
            snapshot.invocation,
            Some(PillQuickControlsInvocation::Focused)
        );
    }

    #[test]
    fn pill_window_mode_has_one_authoritative_geometry_table() {
        assert_eq!(
            pill_window_geometry(PillWindowMode::Compact),
            (
                PILL_WINDOW_COMPACT_WIDTH,
                PILL_WINDOW_COMPACT_HEIGHT,
                PILL_WINDOW_COMPACT_BOTTOM_MARGIN
            )
        );
        assert_eq!(
            pill_window_geometry(PillWindowMode::Review),
            (
                PILL_WINDOW_REVIEW_WIDTH,
                PILL_WINDOW_REVIEW_HEIGHT,
                PILL_WINDOW_REVIEW_BOTTOM_MARGIN
            )
        );
        assert_eq!(
            pill_window_geometry(PillWindowMode::Notice),
            (
                PILL_WINDOW_NOTICE_WIDTH,
                PILL_WINDOW_NOTICE_HEIGHT,
                PILL_WINDOW_NOTICE_BOTTOM_MARGIN
            )
        );
    }

    #[test]
    fn fullscreen_suppression_distinguishes_borderless_from_maximized_windows() {
        let monitor = [0, 0, 1920, 1080];
        assert!(fullscreen_geometry_matches(
            [0, 0, 1920, 1080],
            monitor,
            false
        ));
        assert!(!fullscreen_geometry_matches(
            [-8, -8, 1928, 1048],
            monitor,
            true
        ));
        assert!(!fullscreen_geometry_matches(
            [100, 100, 1400, 900],
            monitor,
            false
        ));
    }
}
