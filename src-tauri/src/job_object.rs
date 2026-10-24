//! Ties helper processes (ASR / faster-whisper / polish workers) to our own
//! lifetime. Windows does not kill children when a parent dies, so a crash or
//! a Task Manager kill would leave workers holding RAM/VRAM. Every helper is
//! assigned to one job object created with KILL_ON_JOB_CLOSE; its handle is
//! never closed, so the OS closes it when VoiceWave exits for any reason and
//! kills everything in the job.

use std::process::Child;

/// Puts `child` in the app-wide kill-on-close job. Call right after spawn.
/// Failure (e.g. the process is already in a job that forbids nesting) is
/// logged and ignored: the explicit worker cleanup still covers clean exits.
pub(crate) fn assign(child: &Child) {
    #[cfg(target_os = "windows")]
    if let Err(reason) = windows::assign(child) {
        tracing::warn!(
            "could not tie helper process {} to the app job: {reason}",
            child.id()
        );
    }
    #[cfg(not(target_os = "windows"))]
    let _ = child;
}

#[cfg(target_os = "windows")]
mod windows {
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;
    use std::sync::OnceLock;
    use windows_sys::Win32::Foundation::{GetLastError, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    /// The job handle as an address (raw handles are not `Sync`). Stays open
    /// until the process ends; that closing is what kills the helpers.
    static JOB: OnceLock<Result<usize, u32>> = OnceLock::new();

    fn create_job() -> Result<usize, u32> {
        // SAFETY: plain Win32 calls; `info` outlives the call and the size matches.
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return Err(GetLastError());
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                std::ptr::addr_of!(info).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if ok == 0 {
                // Leak-free: this handle was never published.
                let code = GetLastError();
                windows_sys::Win32::Foundation::CloseHandle(job);
                return Err(code);
            }
            Ok(job as usize)
        }
    }

    pub(super) fn assign(child: &Child) -> Result<(), String> {
        let job = *JOB
            .get_or_init(create_job)
            .as_ref()
            .map_err(|code| format!("job object setup failed (error {code})"))?;
        // SAFETY: both handles are valid; the child handle is owned by `child`.
        let ok =
            unsafe { AssignProcessToJobObject(job as HANDLE, child.as_raw_handle() as HANDLE) };
        if ok == 0 {
            // SAFETY: reads the calling thread's last error.
            return Err(format!("assign failed (error {})", unsafe {
                GetLastError()
            }));
        }
        Ok(())
    }
}
