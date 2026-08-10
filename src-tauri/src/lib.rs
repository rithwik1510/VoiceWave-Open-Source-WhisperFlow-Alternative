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
pub mod model_manager;
pub mod permissions;
pub mod phase1;
pub mod settings;
pub mod snippet;
pub mod stats;
pub mod transcript;
mod atomic_file;
mod secure_store;

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
use inference::{kill_faster_whisper_workers, llm_polish::kill_polish_worker};
#[cfg(feature = "desktop")]
use insertion::{InsertResult, InsertTextRequest, RecentInsertion, UndoResult};
#[cfg(feature = "desktop")]
use model_manager::{InstalledModel, ModelCatalogItem, ModelDownloadRequest, ModelStatus};
#[cfg(feature = "desktop")]
use permissions::PermissionSnapshot;
#[cfg(feature = "desktop")]
use settings::{
    AppProfileOverrides, CodeModeSettings, DomainPackId, FormatProfile, VoiceWaveSettings,
};
#[cfg(feature = "desktop")]
use snippet::{
    SnippetError, VoiceSnippet, VoiceSnippetReconcileResult, VoiceSnippetSyncRecord,
};
#[cfg(feature = "desktop")]
use state::{DictationMode, VoiceWaveController, VoiceWaveSnapshot};
#[cfg(feature = "desktop")]
use stats::StatsSummary;
#[cfg(feature = "desktop")]
use std::sync::Arc;
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
const TRAY_ID: &str = "voicewave-tray";
#[cfg(feature = "desktop")]
const TRAY_SHOW_ID: &str = "show";
#[cfg(feature = "desktop")]
const TRAY_QUIT_ID: &str = "quit";

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
    if let Err(err) = position_pill_window(
        app,
        &window,
        PILL_WINDOW_COMPACT_WIDTH,
        PILL_WINDOW_COMPACT_HEIGHT,
        PILL_WINDOW_COMPACT_BOTTOM_MARGIN,
    ) {
        eprintln!("voicewave: initial pill positioning deferred: {err}");
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
fn show_pill_for_listening(app: &tauri::AppHandle) -> Result<(), String> {
    let pill = ensure_pill_window(app)?;

    // Showing is the final, non-negotiable operation. Failures in cosmetic
    // setup are logged but must never prevent a valid existing pill window
    // from becoming visible when dictation has been accepted.
    if let Err(err) = pill.set_ignore_cursor_events(true) {
        eprintln!("voicewave: failed to make listening pill click-through: {err}");
    }
    if let Err(err) = pill.set_size(Size::Logical(LogicalSize::new(
        PILL_WINDOW_COMPACT_WIDTH,
        PILL_WINDOW_COMPACT_HEIGHT,
    ))) {
        eprintln!("voicewave: failed to restore listening pill size: {err}");
    }
    if let Err(err) = position_pill_window(
        app,
        &pill,
        PILL_WINDOW_COMPACT_WIDTH,
        PILL_WINDOW_COMPACT_HEIGHT,
        PILL_WINDOW_COMPACT_BOTTOM_MARGIN,
    ) {
        eprintln!("voicewave: failed to re-anchor listening pill: {err}");
    }
    pill.show()
        .map_err(|err| format!("failed to show listening pill: {err}"))
}

#[cfg(feature = "desktop")]
fn state_payload_is_listening(payload: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(payload)
        .ok()
        .and_then(|value| {
            value
                .get("state")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .is_some_and(|state| state == "listening")
}

#[cfg(feature = "desktop")]
fn sync_pill_visibility(app: &tauri::AppHandle, visible: bool) -> Result<(), String> {
    if visible {
        show_pill_for_listening(app)?;
    } else {
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

/// Kill the python side-processes before the app goes away. Both workers are
/// spawned DETACHED_PROCESS, so without this a quit during the first-run model
/// download leaves a python.exe downloading ~487 MB with no UI attached to it.
/// Safe to call more than once (both helpers no-op when nothing is running).
#[cfg(feature = "desktop")]
fn shutdown_inference_workers() {
    kill_faster_whisper_workers();
    kill_polish_worker();
}

#[cfg(feature = "desktop")]
fn configure_system_tray(app: &tauri::AppHandle) -> Result<(), String> {
    let show_item = MenuItemBuilder::with_id(TRAY_SHOW_ID, "Open VoiceWave")
        .build(app)
        .map_err(|err| format!("failed to build tray show item: {err}"))?;
    let quit_item = MenuItemBuilder::with_id(TRAY_QUIT_ID, "Quit VoiceWave")
        .build(app)
        .map_err(|err| format!("failed to build tray quit item: {err}"))?;
    let menu = MenuBuilder::new(app)
        .items(&[&show_item, &quit_item])
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
    runtime: State<'_, RuntimeContext>,
    profile: String,
) -> Result<VoiceWaveSettings, String> {
    runtime
        .controller
        .set_dictation_profile(profile)
        .await
        .map_err(|err| AppError::Controller(err).into())
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

#[cfg(feature = "desktop")]
#[tauri::command]
async fn set_pill_review_mode(app: tauri::AppHandle, review_mode: bool) -> Result<(), String> {
    let pill = ensure_pill_window(&app)?;
    if review_mode {
        let _ = pill.set_ignore_cursor_events(false);
        pill.set_size(Size::Logical(LogicalSize::new(
            PILL_WINDOW_REVIEW_WIDTH,
            PILL_WINDOW_REVIEW_HEIGHT,
        )))
        .map_err(|err| format!("failed to expand floating pill: {err}"))?;
        position_pill_window(
            &app,
            &pill,
            PILL_WINDOW_REVIEW_WIDTH,
            PILL_WINDOW_REVIEW_HEIGHT,
            PILL_WINDOW_REVIEW_BOTTOM_MARGIN,
        )?;
    } else {
        let _ = pill.set_ignore_cursor_events(true);
        pill.set_size(Size::Logical(LogicalSize::new(
            PILL_WINDOW_COMPACT_WIDTH,
            PILL_WINDOW_COMPACT_HEIGHT,
        )))
        .map_err(|err| format!("failed to collapse floating pill: {err}"))?;
        position_pill_window(
            &app,
            &pill,
            PILL_WINDOW_COMPACT_WIDTH,
            PILL_WINDOW_COMPACT_HEIGHT,
            PILL_WINDOW_COMPACT_BOTTOM_MARGIN,
        )?;
    }
    Ok(())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn set_pill_notice_mode(
    app: tauri::AppHandle,
    notice_mode: bool,
    interactive: Option<bool>,
) -> Result<(), String> {
    let pill = ensure_pill_window(&app)?;
    // Plain notices are read-only and stay click-through; rescue notices with
    // an action button need pointer input while expanded (like review mode).
    let interactive = notice_mode && interactive.unwrap_or(false);
    let _ = pill.set_ignore_cursor_events(!interactive);
    let (width, height, margin) = if notice_mode {
        (
            PILL_WINDOW_NOTICE_WIDTH,
            PILL_WINDOW_NOTICE_HEIGHT,
            PILL_WINDOW_NOTICE_BOTTOM_MARGIN,
        )
    } else {
        (
            PILL_WINDOW_COMPACT_WIDTH,
            PILL_WINDOW_COMPACT_HEIGHT,
            PILL_WINDOW_COMPACT_BOTTOM_MARGIN,
        )
    };
    pill.set_size(Size::Logical(LogicalSize::new(width, height)))
        .map_err(|err| format!("failed to resize floating pill for notice: {err}"))?;
    position_pill_window(&app, &pill, width, height, margin)?;
    // A notice must be visible even when the pill was hidden (e.g. idle with
    // HUD hidden between dictations).
    if notice_mode {
        let _ = pill.show();
    }
    Ok(())
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
) -> Result<(), String> {
    runtime
        .controller
        .start_dictation(app, mode.unwrap_or_default())
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
async fn export_dictionary(
    runtime: State<'_, RuntimeContext>,
) -> Result<DictionaryExport, String> {
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
    tauri::Builder::default()
        // Single instance: two event-driven keyboard hooks would BOTH capture
        // and insert every dictation (text lands twice). A second launch must
        // focus the existing window instead. Must be the first plugin
        // registered.
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
        .setup(|app| {
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
            tauri::async_runtime::spawn(async move {
                controller_for_prewarm.prewarm_active_model().await;
                // Warm the optional formatter only after ASR is ready. Running
                // both model preloads concurrently causes avoidable CPU, disk,
                // and memory contention on budget machines.
                controller_for_prewarm
                    .prewarm_active_polish_profile()
                    .await;
            });

            let controller_for_pill_state = controller.clone();
            app.manage(RuntimeContext { controller });

            let app_handle = app.handle().clone();
            let tray_ready = match configure_system_tray(&app_handle) {
                Ok(_) => true,
                Err(err) => {
                    eprintln!("voicewave: tray setup failed, falling back to close-to-exit: {err}");
                    false
                }
            };
            configure_main_window_close_behavior(&app_handle, tray_ready);
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
                    if !state_payload_is_listening(event.payload()) {
                        return;
                    }
                    let handle = listening_handle.clone();
                    let controller = controller_for_pill_state.clone();
                    tauri::async_runtime::spawn(async move {
                        // Preserve the explicit user preference to disable
                        // the HUD; otherwise every Listening transition must
                        // end with a visible compact pill.
                        if controller.load_settings().await.show_floating_hud {
                            if let Err(err) = show_pill_for_listening(&handle) {
                                eprintln!("voicewave: listening pill recovery failed: {err}");
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
            set_pill_review_mode,
            set_pill_notice_mode,
            copy_text_to_clipboard,
            get_diagnostics_status,
            set_diagnostics_opt_in,
            export_diagnostics_bundle,
            start_dictation,
            cancel_dictation,
            stop_dictation,
            load_hotkey_config,
            update_hotkey_config,
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
    use super::state_payload_is_listening;

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
}
