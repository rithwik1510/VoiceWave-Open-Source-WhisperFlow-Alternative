#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// Primary single-instance guard, deliberately ahead of every line of Tauri
/// code.
///
/// `CreateMutexW` is atomic in the kernel, so two launches in the same instant
/// have exactly one winner. The loser focuses the winner with non-blocking
/// calls only and exits before it ever touches WebView2. That ordering is the
/// point: `tauri-plugin-single-instance` (still registered in `lib.rs::run` as
/// a second line of defence) notifies the winner with a blocking
/// `SendMessage`, so when v0.6.0 lost this race both processes wedged - the
/// loser blocked on a winner whose main thread was itself stuck contending for
/// the shared WebView2 user-data folder, and the app looked completely dead.
#[cfg(all(windows, feature = "desktop"))]
fn claim_single_instance_or_exit() {
    use windows_sys::Win32::{
        Foundation::{GetLastError, ERROR_ALREADY_EXISTS, WAIT_ABANDONED, WAIT_OBJECT_0},
        System::Threading::{CreateMutexW, WaitForSingleObject},
        UI::WindowsAndMessaging::{FindWindowW, SetForegroundWindow, ShowWindowAsync, SW_RESTORE},
    };

    let name = wide("VoiceWave-single-instance");
    // SAFETY: `name` is a valid nul-terminated UTF-16 buffer that outlives the
    // call; a null security descriptor gives the default (per-session) ACL.
    let handle = unsafe { CreateMutexW(std::ptr::null(), 1, name.as_ptr()) };
    if handle.is_null() {
        // We cannot tell winner from loser. Booting is the safer failure:
        // worst case is the pre-existing duplicate-instance behaviour, and the
        // hotkey-hook mutex still keeps a single keyboard hook system-wide.
        return;
    }
    if unsafe { GetLastError() } != ERROR_ALREADY_EXISTS {
        // Winner. `handle` is intentionally leaked: the mutex must live for
        // the whole process lifetime, and the kernel closes it on exit.
        return;
    }

    // Loser path. Best-effort, strictly non-blocking focus of the running
    // instance - `ShowWindowAsync` posts rather than sends, and
    // `SetForegroundWindow` returns immediately whether or not the foreground
    // lock lets it through. Never a plain `SendMessage` here.
    let title = wide("VoiceWave Local Core");
    // SAFETY: both pointers are valid nul-terminated UTF-16 buffers, and the
    // HWND is only used for two calls that tolerate a stale handle.
    let window = unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) };
    if !window.is_null() {
        unsafe {
            let _ = ShowWindowAsync(window, SW_RESTORE);
            let _ = SetForegroundWindow(window);
        }
        std::process::exit(0);
    }

    // The mutex exists but no main window does: either the previous process is
    // mid-shutdown (an updater relaunch spawns us before it exits) or it is
    // wedged. Exiting outright in that window is exactly the "app is dead"
    // symptom, so give the old process a bounded moment to release the mutex
    // and boot as the winner if it does.
    // SAFETY: `handle` is a live mutex handle owned by this call.
    let wait = unsafe { WaitForSingleObject(handle, 1_500) };
    if wait == WAIT_OBJECT_0 || wait == WAIT_ABANDONED {
        // Acquired - we are the winner now; leak the handle as above.
        return;
    }
    std::process::exit(0);
}

#[cfg(all(windows, feature = "desktop"))]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(feature = "desktop")]
fn main() {
    #[cfg(windows)]
    claim_single_instance_or_exit();

    voicewave_core_lib::run();
}

#[cfg(not(feature = "desktop"))]
fn main() {
    eprintln!("VoiceWave desktop runtime disabled (built without 'desktop' feature).");
}
