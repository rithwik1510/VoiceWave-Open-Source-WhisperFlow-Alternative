//! Windows-native click target for the otherwise click-through status pill.
//!
//! The visual pill remains a transparent Tauri/WebView window. This tiny
//! top-level Win32 window receives only the initial user click and never
//! activates VoiceWave, avoiding WebView2 child-HWND focus behavior.

#[cfg(target_os = "windows")]
mod windows {
    use std::{
        ptr::{null, null_mut},
        sync::{
            atomic::{AtomicBool, Ordering},
            mpsc::{self, Sender},
            Arc,
        },
        thread,
        time::Duration,
    };
    use windows_sys::Win32::UI::Input::Pointer::GetPointerType;
    use windows_sys::Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
            LoadCursorW, PostMessageW, PostQuitMessage, RegisterClassW, SetLayeredWindowAttributes,
            SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage, UnregisterClassW,
            CREATESTRUCTW, GWLP_USERDATA, HWND_TOPMOST, IDC_ARROW, LWA_ALPHA, MA_NOACTIVATE, MSG,
            PT_TOUCH, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_SHOWWINDOW, SW_HIDE, WM_CLOSE,
            WM_DESTROY, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_NCCREATE, WM_NCDESTROY, WM_POINTERUP,
            WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
        },
    };

    const CLASS_NAME: &str = "VoiceWavePillHitTarget";

    struct WindowContext {
        click_tx: Sender<()>,
        armed: Arc<AtomicBool>,
    }

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_NCCREATE => {
                let create = &*(lparam as *const CREATESTRUCTW);
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
                1
            }
            WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
            WM_LBUTTONUP => {
                let context = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const WindowContext;
                if !context.is_null() && (*context).armed.swap(false, Ordering::AcqRel) {
                    let _ = (*context).click_tx.send(());
                }
                0
            }
            WM_POINTERUP => {
                let context = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const WindowContext;
                let pointer_id = (wparam & 0xFFFF) as u32;
                let mut pointer_type = 0;
                let is_touch =
                    GetPointerType(pointer_id, &mut pointer_type) != 0 && pointer_type == PT_TOUCH;
                if is_touch && !context.is_null() && (*context).armed.swap(false, Ordering::AcqRel)
                {
                    let _ = (*context).click_tx.send(());
                }
                0
            }
            WM_CLOSE => {
                DestroyWindow(hwnd);
                0
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            WM_NCDESTROY => {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }

    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW;

    #[derive(Debug)]
    pub struct PillHitTarget {
        hwnd: isize,
        armed: Arc<AtomicBool>,
    }

    impl PillHitTarget {
        pub fn spawn(on_click: impl Fn() + Send + 'static) -> Result<Self, String> {
            let (click_tx, click_rx) = mpsc::channel::<()>();
            let armed = Arc::new(AtomicBool::new(false));
            let window_armed = Arc::clone(&armed);
            thread::Builder::new()
                .name("voicewave-pill-hit-callback".to_string())
                .spawn(move || {
                    while click_rx.recv().is_ok() {
                        on_click();
                    }
                })
                .map_err(|err| format!("failed to start pill click callback: {err}"))?;

            let (ready_tx, ready_rx) = mpsc::sync_channel(1);
            thread::Builder::new()
                .name("voicewave-pill-hit-window".to_string())
                .spawn(move || unsafe {
                    let instance = GetModuleHandleW(null());
                    let class_name = wide(CLASS_NAME);
                    let mut class: WNDCLASSW = std::mem::zeroed();
                    class.lpfnWndProc = Some(window_proc);
                    class.hInstance = instance;
                    class.hCursor = LoadCursorW(null_mut(), IDC_ARROW);
                    class.lpszClassName = class_name.as_ptr();
                    if RegisterClassW(&class) == 0 {
                        let _ = ready_tx.send(Err(
                            "failed to register native pill hit-target class".to_string(),
                        ));
                        return;
                    }

                    let context = Box::into_raw(Box::new(WindowContext {
                        click_tx,
                        armed: window_armed,
                    }));
                    let hwnd = CreateWindowExW(
                        WS_EX_LAYERED | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
                        class_name.as_ptr(),
                        class_name.as_ptr(),
                        WS_POPUP,
                        0,
                        0,
                        1,
                        1,
                        null_mut(),
                        null_mut(),
                        instance,
                        context.cast(),
                    );
                    if hwnd.is_null() {
                        drop(Box::from_raw(context));
                        UnregisterClassW(class_name.as_ptr(), instance);
                        let _ = ready_tx.send(Err(
                            "failed to create native pill hit-target window".to_string(),
                        ));
                        return;
                    }

                    // Alpha 1 is visually imperceptible but still participates
                    // in hit testing. Alpha 0 layered windows are skipped on
                    // some Windows builds and would make clicks unreliable.
                    if SetLayeredWindowAttributes(hwnd, 0, 1, LWA_ALPHA) == 0 {
                        DestroyWindow(hwnd);
                        drop(Box::from_raw(context));
                        UnregisterClassW(class_name.as_ptr(), instance);
                        let _ =
                            ready_tx
                                .send(Err("failed to configure native pill hit-target opacity"
                                    .to_string()));
                        return;
                    }

                    let _ = ready_tx.send(Ok(hwnd as isize));
                    let mut message: MSG = std::mem::zeroed();
                    while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
                        TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                    drop(Box::from_raw(context));
                    UnregisterClassW(class_name.as_ptr(), instance);
                })
                .map_err(|err| format!("failed to start pill hit-target window: {err}"))?;

            let hwnd = ready_rx
                .recv_timeout(Duration::from_secs(3))
                .map_err(|_| "timed out creating native pill hit target".to_string())??;
            Ok(Self { hwnd, armed })
        }

        pub fn show(&self, x: i32, y: i32, width: i32, height: i32) -> Result<(), String> {
            self.armed.store(true, Ordering::Release);
            let ok = unsafe {
                SetWindowPos(
                    self.hwnd as HWND,
                    HWND_TOPMOST,
                    x,
                    y,
                    width,
                    height,
                    SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_SHOWWINDOW,
                )
            };
            if ok == 0 {
                self.armed.store(false, Ordering::Release);
                return Err("failed to position native pill hit target".to_string());
            }
            Ok(())
        }

        pub fn hide(&self) {
            self.armed.store(false, Ordering::Release);
            unsafe {
                ShowWindow(self.hwnd as HWND, SW_HIDE);
            }
        }
    }

    impl Drop for PillHitTarget {
        fn drop(&mut self) {
            unsafe {
                PostMessageW(self.hwnd as HWND, WM_CLOSE, 0, 0);
            }
        }
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }
}

#[cfg(target_os = "windows")]
pub use windows::PillHitTarget;

#[cfg(not(target_os = "windows"))]
#[derive(Debug)]
pub struct PillHitTarget;

#[cfg(not(target_os = "windows"))]
impl PillHitTarget {
    pub fn spawn(_on_click: impl Fn() + Send + 'static) -> Result<Self, String> {
        Err("native pill hit target is Windows-only".to_string())
    }

    pub fn show(&self, _x: i32, _y: i32, _width: i32, _height: i32) -> Result<(), String> {
        Err("native pill hit target is Windows-only".to_string())
    }

    pub fn hide(&self) {}
}
