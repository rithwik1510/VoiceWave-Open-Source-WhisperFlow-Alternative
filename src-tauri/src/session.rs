//! How the previous run ended (plan 024 section 5). Each run keeps
//! `<data_dir>/session.json` while it's alive and deletes it on a clean exit,
//! so a file found at startup means the last run was cut short; what it says
//! tells a crash from a panic, a Windows restart, or an update that did or
//! didn't land. Local only: the result reaches analytics through
//! `app_launched`, the UI through `SupportContext`, and an unclean run's file
//! goes into the support file.
//!
//! Writes stay off the dictation path: `set_phase` only updates memory and
//! wakes the heartbeat thread, which writes at most every 10 s and otherwise
//! every 5 min.

use crate::analytics::{PreviousExit, Semver, SessionPhase};
use crate::atomic_file::{self, StoreLoad};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{mpsc, Mutex, MutexGuard, OnceLock, PoisonError},
    time::{Duration, Instant},
};

const FILE: &str = "session.json";
const HEARTBEAT: Duration = Duration::from_secs(5 * 60);
const MIN_WRITE_GAP: Duration = Duration::from_secs(10);
/// Two runs in the same Windows boot agree on the boot time within seconds.
const SAME_BOOT_TOLERANCE_MS: u64 = 120_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionFile {
    v: u32,
    app_version: String,
    started_at_ms: u64,
    last_alive_ms: u64,
    /// When Windows booted, to the minute.
    boot_at_ms: u64,
    phase: SessionPhase,
    panicked: bool,
    update_target: Option<Semver>,
}

/// What the previous run's session file said. `exit` is `None` without a
/// file: a clean exit deleted it, or this is the install's first run
/// (analytics tells the two apart).
#[derive(Debug, Clone, Copy, Default)]
pub struct PreviousRun {
    pub exit: Option<PreviousExit>,
    pub phase: Option<SessionPhase>,
    pub uptime_min: Option<u64>,
    /// The previous run was installing this version, and it didn't land.
    pub update_not_applied: Option<Semver>,
}

impl PreviousRun {
    /// A crash or a panic, as opposed to a restart, an update or a clean exit.
    pub fn unclean(&self) -> bool {
        matches!(
            self.exit,
            Some(PreviousExit::Crash | PreviousExit::AfterPanic)
        )
    }
}

struct State {
    path: PathBuf,
    file: SessionFile,
    /// Set by `clear`: nothing may write the file again.
    closed: bool,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
/// Held across each file write and the final delete, so a late write can't
/// bring the file back after `clear`. Never taken while holding `STATE`.
static WRITE: Mutex<()> = Mutex::new(());
static WAKE: OnceLock<mpsc::Sender<()>> = OnceLock::new();
static PREVIOUS: OnceLock<PreviousRun> = OnceLock::new();
/// The previous run's file, kept for the support file when that run crashed
/// or panicked: this run's file has replaced it on disk.
static UNCLEAN_FILE: OnceLock<Vec<u8>> = OnceLock::new();

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn now_ms() -> u64 {
    u64::try_from(chrono::Utc::now().timestamp_millis()).unwrap_or_default()
}

/// Reads the previous run's file, then starts this run's. Call once, early
/// in startup (after the single-instance claim and `support::init`).
pub fn start() {
    let Some(dir) = directories::ProjectDirs::from("com", "voicewave", "localcore")
        .map(|dirs| dirs.data_dir().to_path_buf())
    else {
        return;
    };
    start_at(dir.join(FILE));
}

fn start_at(path: PathBuf) {
    let now = now_ms();
    let boot_at_ms = boot_at_ms(now);
    let stored = previous_file(&path);
    let previous = classify(stored.as_ref(), boot_at_ms);
    if previous.exit.is_some() {
        tracing::info!("previous run: {previous:?}");
    }
    if let Some(file) = stored.filter(|_| previous.unclean()) {
        if let Ok(raw) = serde_json::to_vec_pretty(&file) {
            let _ = UNCLEAN_FILE.set(raw);
        }
    }
    let _ = PREVIOUS.set(previous);

    *lock(&STATE) = Some(State {
        path,
        file: SessionFile {
            v: 1,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            started_at_ms: now,
            last_alive_ms: now,
            boot_at_ms,
            phase: SessionPhase::Startup,
            panicked: false,
            update_target: None,
        },
        closed: false,
    });
    write_now();

    let (wake, woken) = mpsc::channel();
    if WAKE.set(wake).is_ok() {
        if let Err(err) = std::thread::Builder::new()
            .name("session-heartbeat".to_string())
            .spawn(move || heartbeat(woken))
        {
            tracing::warn!("session heartbeat unavailable: {err}");
        }
    }
}

/// How the previous run ended; the default (no file) before `start`.
pub fn previous() -> PreviousRun {
    PREVIOUS.get().copied().unwrap_or_default()
}

/// The previous run's session file, only when that run crashed or panicked.
pub fn unclean_previous_file() -> Option<&'static [u8]> {
    UNCLEAN_FILE.get().map(Vec::as_slice)
}

/// A corrupt or unreadable file counts as absent. Single instance (claimed
/// before `start`) means no other run can still own the file.
fn previous_file(path: &Path) -> Option<SessionFile> {
    match atomic_file::load_with_recovery(path, "session", |raw| {
        serde_json::from_str::<SessionFile>(raw)
    }) {
        StoreLoad::Loaded(file) | StoreLoad::Recovered(file) => Some(file),
        StoreLoad::Missing | StoreLoad::Reset => None,
    }
}

/// Plan section 5's table, in order.
fn classify(previous: Option<&SessionFile>, boot_at_ms: u64) -> PreviousRun {
    let Some(previous) = previous else {
        return PreviousRun::default();
    };
    let mut run = PreviousRun {
        exit: None,
        phase: Some(previous.phase),
        uptime_min: Some(
            previous
                .last_alive_ms
                .saturating_sub(previous.started_at_ms)
                / 60_000,
        ),
        update_not_applied: None,
    };
    let exit = if previous.phase == SessionPhase::Updating {
        let from = Semver::parse(&previous.app_version);
        let current = Semver::current();
        if from.is_valid() && current.is_valid() && from < current {
            PreviousExit::Update
        } else {
            if from == current {
                run.update_not_applied = Some(previous.update_target.unwrap_or(Semver::parse("")));
            }
            PreviousExit::Clean
        }
    } else if previous.panicked {
        PreviousExit::AfterPanic
    } else if previous.boot_at_ms.abs_diff(boot_at_ms) > SAME_BOOT_TOLERANCE_MS {
        PreviousExit::SystemRestart
    } else {
        PreviousExit::Crash
    };
    run.exit = Some(exit);
    run
}

/// Dictating while capture runs, idle otherwise. Only touches memory; the
/// heartbeat thread writes it. An update in progress keeps its phase.
pub fn set_phase(phase: SessionPhase) {
    {
        let mut state = lock(&STATE);
        let Some(state) = state.as_mut() else {
            return;
        };
        if state.closed || state.file.phase == phase || state.file.phase == SessionPhase::Updating {
            return;
        }
        state.file.phase = phase;
    }
    if let Some(wake) = WAKE.get() {
        let _ = wake.send(());
    }
}

/// The updater has downloaded `target` and is about to run its installer.
/// Written at once: the installer may end this process at any moment.
pub fn mark_update_installing(target: Semver) {
    if let Some(state) = lock(&STATE).as_mut() {
        state.file.phase = SessionPhase::Updating;
        state.file.update_target = Some(target);
    }
    write_now();
}

/// The installer failed to start.
pub fn clear_update_installing() {
    if let Some(state) = lock(&STATE).as_mut() {
        if state.file.phase == SessionPhase::Updating {
            state.file.phase = SessionPhase::Idle;
        }
        state.file.update_target = None;
    }
    write_now();
}

/// From the panic hook: best effort, and never panics or waits long. A
/// panic inside a write (holding a lock) just skips the mark.
pub fn mark_panicked() {
    for _ in 0..10 {
        let Ok(_write) = WRITE.try_lock() else {
            std::thread::sleep(Duration::from_millis(5));
            continue;
        };
        let Ok(mut state) = STATE.try_lock() else {
            return;
        };
        let Some(state) = state.as_mut() else {
            return;
        };
        if state.closed || state.file.panicked {
            return;
        }
        state.file.panicked = true;
        state.file.last_alive_ms = now_ms();
        save(&state.path, &state.file);
        return;
    }
}

/// A clean exit: delete the file. While an update installs it stays, so the
/// next launch can tell whether the update landed.
pub fn clear() {
    let (path, keep) = {
        let mut state = lock(&STATE);
        let Some(state) = state.as_mut() else {
            return;
        };
        state.closed = true;
        (
            state.path.clone(),
            state.file.phase == SessionPhase::Updating,
        )
    };
    if keep {
        return;
    }
    let _write = lock(&WRITE);
    if let Err(err) = std::fs::remove_file(&path) {
        if err.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!("couldn't delete the session file: {err}");
        }
    }
}

/// Rewrites the file with `lastAliveMs = now`, unless the run has closed.
fn write_now() {
    let _write = lock(&WRITE);
    let (path, file) = {
        let mut state = lock(&STATE);
        let Some(state) = state.as_mut() else {
            return;
        };
        if state.closed {
            return;
        }
        state.file.last_alive_ms = now_ms();
        (state.path.clone(), state.file.clone())
    };
    save(&path, &file);
}

fn save(path: &Path, file: &SessionFile) {
    match serde_json::to_vec(file) {
        Ok(raw) => {
            if let Err(err) = atomic_file::atomic_write(path, &raw) {
                tracing::warn!("couldn't save the session file: {err}");
            }
        }
        Err(err) => tracing::warn!("couldn't encode the session file: {err}"),
    }
}

fn heartbeat(woken: mpsc::Receiver<()>) {
    let mut last_write = Instant::now();
    loop {
        match woken.recv_timeout(HEARTBEAT) {
            Ok(()) => {
                // A phase change: at most one write per `MIN_WRITE_GAP`, and
                // the changes in between collapse into it.
                std::thread::sleep(MIN_WRITE_GAP.saturating_sub(last_write.elapsed()));
                while woken.try_recv().is_ok() {}
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
        if lock(&STATE).as_ref().is_none_or(|state| state.closed) {
            return;
        }
        write_now();
        last_write = Instant::now();
    }
}

/// `now - GetTickCount64()`, rounded to the minute.
#[cfg(windows)]
fn boot_at_ms(now: u64) -> u64 {
    // SAFETY: GetTickCount64 has no arguments and cannot fail.
    let uptime = unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() };
    let boot = now.saturating_sub(uptime);
    (boot + 30_000) / 60_000 * 60_000
}

#[cfg(not(windows))]
fn boot_at_ms(_now: u64) -> u64 {
    0
}
