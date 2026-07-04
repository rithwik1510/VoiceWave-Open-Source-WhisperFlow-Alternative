//! OS-level microphone input volume guard.
//!
//! Windows apps with microphone access (browser WebRTC auto-gain, call apps,
//! vendor audio utilities) are allowed to change the *system* capture volume
//! and routinely leave it lowered. Whisper then receives quiet audio and both
//! models degrade at once — observed in the field twice (2026-05 at 47%,
//! 2026-07 at 40%). This module reads (and, only when the user opts in,
//! restores) the endpoint volume of the default capture device so that
//! failure mode surfaces as an explicit pill notice instead of silently
//! "the model got worse".
//!
//! Limitation: the query targets the *default* capture endpoint. When the
//! user selects a non-default input device in settings, the guard still
//! reads the default endpoint; matching arbitrary cpal names to MMDevice
//! endpoints needs IPropertyStore plumbing that is not worth the surface
//! yet — the shipped device picker defaults to the system default mic.

use serde::{Deserialize, Serialize};

/// Input volumes below this scalar trigger the guard. Chosen so deliberate
/// "slightly lowered" setups (80-90%) stay quiet while the observed
/// regression band (40-50%) always trips it.
pub const LOW_INPUT_VOLUME_THRESHOLD: f32 = 0.70;

/// Target applied by auto-restore. Full scale matches the level the mic was
/// calibrated at when dictation quality was validated.
pub const RESTORE_TARGET_SCALAR: f32 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InputVolumeReading {
    /// Master volume scalar of the default capture endpoint, 0.0..=1.0.
    pub scalar: f32,
    pub muted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum MicVolumeGuardMode {
    Off,
    /// Surface a pill notice when the input volume is low or muted. Never
    /// touches OS state — this is the trust-preserving default.
    #[default]
    Warn,
    /// Restore the input volume to 100% (and notify that it happened).
    /// Muted stays untouched even here: unmuting is a stronger statement of
    /// user intent than a volume slider and must stay manual.
    AutoRestore,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MicGuardPlan {
    Silent,
    WarnMuted,
    WarnLow { volume_percent: u8 },
    Restore { from_percent: u8 },
}

pub fn plan_mic_guard(mode: MicVolumeGuardMode, reading: InputVolumeReading) -> MicGuardPlan {
    if mode == MicVolumeGuardMode::Off {
        return MicGuardPlan::Silent;
    }
    if reading.muted {
        return MicGuardPlan::WarnMuted;
    }
    if reading.scalar >= LOW_INPUT_VOLUME_THRESHOLD {
        return MicGuardPlan::Silent;
    }
    let percent = (reading.scalar * 100.0).round().clamp(0.0, 100.0) as u8;
    match mode {
        MicVolumeGuardMode::Warn => MicGuardPlan::WarnLow {
            volume_percent: percent,
        },
        MicVolumeGuardMode::AutoRestore => MicGuardPlan::Restore {
            from_percent: percent,
        },
        MicVolumeGuardMode::Off => MicGuardPlan::Silent,
    }
}

#[cfg(target_os = "windows")]
mod platform {
    //! Minimal hand-rolled Core Audio COM bindings. `windows-sys` ships no
    //! COM vtables (that is the full `windows` crate's territory), and
    //! pulling that crate in for three method calls is not worth the
    //! dependency. Vtable layouts mirror mmdeviceapi.h / endpointvolume.h;
    //! methods this module never calls are `usize` placeholders, which are
    //! pointer-sized and keep the layout intact.

    use super::{InputVolumeReading, RESTORE_TARGET_SCALAR};
    use std::ffi::c_void;
    use std::ptr;
    use windows_sys::core::GUID;
    use windows_sys::Win32::Foundation::{RPC_E_CHANGED_MODE, S_FALSE};
    use windows_sys::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
    };

    const CLSID_MM_DEVICE_ENUMERATOR: GUID =
        GUID::from_u128(0xbcde0395_e52f_467c_8e3d_c4579291692e);
    const IID_IMM_DEVICE_ENUMERATOR: GUID = GUID::from_u128(0xa95664d2_9614_4f35_a746_de8db63617e6);
    const IID_IAUDIO_ENDPOINT_VOLUME: GUID =
        GUID::from_u128(0x5cdf2c82_841e_4546_9722_0cf74078229a);

    /// EDataFlow::eCapture / ERole::eConsole from mmdeviceapi.h.
    const E_DATA_FLOW_CAPTURE: i32 = 1;
    const E_ROLE_CONSOLE: i32 = 0;

    type Hresult = i32;

    #[repr(C)]
    struct IUnknownVtbl {
        query_interface:
            unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> Hresult,
        add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
        release: unsafe extern "system" fn(*mut c_void) -> u32,
    }

    /// IMMDeviceEnumerator: EnumAudioEndpoints, GetDefaultAudioEndpoint,
    /// GetDevice, Register/UnregisterEndpointNotificationCallback.
    #[repr(C)]
    struct IMMDeviceEnumeratorVtbl {
        base: IUnknownVtbl,
        enum_audio_endpoints: usize,
        get_default_audio_endpoint:
            unsafe extern "system" fn(*mut c_void, i32, i32, *mut *mut c_void) -> Hresult,
    }

    /// IMMDevice: Activate, OpenPropertyStore, GetId, GetState.
    #[repr(C)]
    struct IMMDeviceVtbl {
        base: IUnknownVtbl,
        activate: unsafe extern "system" fn(
            *mut c_void,
            *const GUID,
            u32,
            *const c_void,
            *mut *mut c_void,
        ) -> Hresult,
    }

    /// IAudioEndpointVolume, endpointvolume.h order.
    #[repr(C)]
    struct IAudioEndpointVolumeVtbl {
        base: IUnknownVtbl,
        register_control_change_notify: usize,
        unregister_control_change_notify: usize,
        get_channel_count: usize,
        set_master_volume_level: usize,
        set_master_volume_level_scalar:
            unsafe extern "system" fn(*mut c_void, f32, *const GUID) -> Hresult,
        get_master_volume_level: usize,
        get_master_volume_level_scalar:
            unsafe extern "system" fn(*mut c_void, *mut f32) -> Hresult,
        set_channel_volume_level: usize,
        set_channel_volume_level_scalar: usize,
        get_channel_volume_level: usize,
        get_channel_volume_level_scalar: usize,
        set_mute: usize,
        get_mute: unsafe extern "system" fn(*mut c_void, *mut i32) -> Hresult,
    }

    unsafe fn vtbl<T>(com_object: *mut c_void) -> *const T {
        *(com_object as *mut *const T)
    }

    unsafe fn release(com_object: *mut c_void) {
        let unknown = vtbl::<IUnknownVtbl>(com_object);
        ((*unknown).release)(com_object);
    }

    struct ComGuard {
        should_uninit: bool,
    }

    impl ComGuard {
        fn init() -> Result<Self, String> {
            // S_OK / S_FALSE both mean COM is usable on this thread; S_FALSE
            // (already initialized) still requires a balancing CoUninitialize.
            // RPC_E_CHANGED_MODE means the thread is STA — usable, no balance.
            let hr = unsafe { CoInitializeEx(ptr::null(), COINIT_MULTITHREADED as u32) };
            if hr == 0 || hr == S_FALSE {
                Ok(Self {
                    should_uninit: true,
                })
            } else if hr == RPC_E_CHANGED_MODE {
                Ok(Self {
                    should_uninit: false,
                })
            } else {
                Err(format!("CoInitializeEx failed: 0x{hr:08x}"))
            }
        }
    }

    impl Drop for ComGuard {
        fn drop(&mut self) {
            if self.should_uninit {
                unsafe { CoUninitialize() };
            }
        }
    }

    struct EndpointVolume {
        _guard: ComGuard,
        enumerator: *mut c_void,
        device: *mut c_void,
        volume: *mut c_void,
    }

    impl EndpointVolume {
        fn open_default_capture() -> Result<Self, String> {
            let guard = ComGuard::init()?;

            let mut enumerator: *mut c_void = ptr::null_mut();
            let hr = unsafe {
                CoCreateInstance(
                    &CLSID_MM_DEVICE_ENUMERATOR,
                    ptr::null_mut(),
                    CLSCTX_ALL,
                    &IID_IMM_DEVICE_ENUMERATOR,
                    &mut enumerator,
                )
            };
            if hr != 0 || enumerator.is_null() {
                return Err(format!("MMDeviceEnumerator create failed: 0x{hr:08x}"));
            }

            let mut device: *mut c_void = ptr::null_mut();
            let hr = unsafe {
                ((*vtbl::<IMMDeviceEnumeratorVtbl>(enumerator)).get_default_audio_endpoint)(
                    enumerator,
                    E_DATA_FLOW_CAPTURE,
                    E_ROLE_CONSOLE,
                    &mut device,
                )
            };
            if hr != 0 || device.is_null() {
                unsafe { release(enumerator) };
                return Err(format!("GetDefaultAudioEndpoint failed: 0x{hr:08x}"));
            }

            let mut volume: *mut c_void = ptr::null_mut();
            let hr = unsafe {
                ((*vtbl::<IMMDeviceVtbl>(device)).activate)(
                    device,
                    &IID_IAUDIO_ENDPOINT_VOLUME,
                    CLSCTX_ALL,
                    ptr::null(),
                    &mut volume,
                )
            };
            if hr != 0 || volume.is_null() {
                unsafe {
                    release(device);
                    release(enumerator);
                }
                return Err(format!("IAudioEndpointVolume activate failed: 0x{hr:08x}"));
            }

            Ok(Self {
                _guard: guard,
                enumerator,
                device,
                volume,
            })
        }
    }

    impl Drop for EndpointVolume {
        fn drop(&mut self) {
            unsafe {
                release(self.volume);
                release(self.device);
                release(self.enumerator);
            }
            // `_guard` (CoUninitialize) drops after the COM pointers above.
        }
    }

    pub fn read_default_input_volume() -> Result<InputVolumeReading, String> {
        let endpoint = EndpointVolume::open_default_capture()?;
        let mut scalar: f32 = 0.0;
        let hr = unsafe {
            ((*vtbl::<IAudioEndpointVolumeVtbl>(endpoint.volume)).get_master_volume_level_scalar)(
                endpoint.volume,
                &mut scalar,
            )
        };
        if hr != 0 {
            return Err(format!("GetMasterVolumeLevelScalar failed: 0x{hr:08x}"));
        }
        let mut muted: i32 = 0;
        let hr = unsafe {
            ((*vtbl::<IAudioEndpointVolumeVtbl>(endpoint.volume)).get_mute)(
                endpoint.volume,
                &mut muted,
            )
        };
        if hr != 0 {
            return Err(format!("GetMute failed: 0x{hr:08x}"));
        }
        Ok(InputVolumeReading {
            scalar: scalar.clamp(0.0, 1.0),
            muted: muted != 0,
        })
    }

    pub fn restore_default_input_volume() -> Result<(), String> {
        let endpoint = EndpointVolume::open_default_capture()?;
        let hr = unsafe {
            ((*vtbl::<IAudioEndpointVolumeVtbl>(endpoint.volume)).set_master_volume_level_scalar)(
                endpoint.volume,
                RESTORE_TARGET_SCALAR,
                ptr::null(),
            )
        };
        if hr != 0 {
            return Err(format!("SetMasterVolumeLevelScalar failed: 0x{hr:08x}"));
        }
        Ok(())
    }
}

#[cfg(target_os = "windows")]
pub use platform::{read_default_input_volume, restore_default_input_volume};

#[cfg(not(target_os = "windows"))]
pub fn read_default_input_volume() -> Result<InputVolumeReading, String> {
    Err("input volume guard is Windows-only".to_string())
}

#[cfg(not(target_os = "windows"))]
pub fn restore_default_input_volume() -> Result<(), String> {
    Err("input volume guard is Windows-only".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(scalar: f32, muted: bool) -> InputVolumeReading {
        InputVolumeReading { scalar, muted }
    }

    #[test]
    fn off_mode_never_acts() {
        assert_eq!(
            plan_mic_guard(MicVolumeGuardMode::Off, reading(0.1, false)),
            MicGuardPlan::Silent
        );
        assert_eq!(
            plan_mic_guard(MicVolumeGuardMode::Off, reading(0.1, true)),
            MicGuardPlan::Silent
        );
    }

    #[test]
    fn healthy_volume_is_silent() {
        for mode in [MicVolumeGuardMode::Warn, MicVolumeGuardMode::AutoRestore] {
            assert_eq!(plan_mic_guard(mode, reading(1.0, false)), MicGuardPlan::Silent);
            assert_eq!(plan_mic_guard(mode, reading(0.70, false)), MicGuardPlan::Silent);
            assert_eq!(plan_mic_guard(mode, reading(0.85, false)), MicGuardPlan::Silent);
        }
    }

    #[test]
    fn low_volume_warns_in_warn_mode() {
        assert_eq!(
            plan_mic_guard(MicVolumeGuardMode::Warn, reading(0.40, false)),
            MicGuardPlan::WarnLow { volume_percent: 40 }
        );
    }

    #[test]
    fn low_volume_restores_in_auto_restore_mode() {
        assert_eq!(
            plan_mic_guard(MicVolumeGuardMode::AutoRestore, reading(0.47, false)),
            MicGuardPlan::Restore { from_percent: 47 }
        );
    }

    #[test]
    fn muted_always_warns_never_auto_unmutes() {
        assert_eq!(
            plan_mic_guard(MicVolumeGuardMode::Warn, reading(1.0, true)),
            MicGuardPlan::WarnMuted
        );
        assert_eq!(
            plan_mic_guard(MicVolumeGuardMode::AutoRestore, reading(1.0, true)),
            MicGuardPlan::WarnMuted
        );
    }

    /// Live smoke test against the real Core Audio stack — validates the
    /// hand-rolled vtable layouts. Run explicitly:
    /// `cargo test --lib input_volume_live_smoke -- --ignored --nocapture`
    #[test]
    #[ignore]
    #[cfg(target_os = "windows")]
    fn input_volume_live_smoke() {
        let reading = read_default_input_volume().expect("read volume");
        println!("default capture endpoint: {reading:?}");
        assert!((0.0..=1.0).contains(&reading.scalar));
    }

    #[test]
    fn guard_mode_serde_uses_camel_case_tokens() {
        assert_eq!(
            serde_json::to_string(&MicVolumeGuardMode::AutoRestore).unwrap(),
            "\"autoRestore\""
        );
        assert_eq!(
            serde_json::from_str::<MicVolumeGuardMode>("\"warn\"").unwrap(),
            MicVolumeGuardMode::Warn
        );
        assert_eq!(MicVolumeGuardMode::default(), MicVolumeGuardMode::Warn);
    }
}
