# Global Hotkey Evidence (Windows)

Status: Complete

## Goal

Capture OS-level global hotkey registration evidence for Phase IV hardening.

## Evidence

1. Runtime monitor startup log captured from desktop run:
   - `docs/phase4/artifacts/global-hotkey-runtime-smoke-2026-07-11.log`
   - verifies the Windows low-level keyboard hook installs and its message
     loop shuts down cleanly.
2. Runtime implementation path:
   - `src-tauri/src/state.rs` (`ensure_hotkey_runtime_monitor`)
   - receives pressed/released/triggered edges from the event-driven global
     keyboard hook rather than polling OS key state.
3. Failure-recovery path:
   - invalid hotkey configuration fallback to defaults remains active at startup in `src-tauri/src/state.rs` constructor path.
   - hotkey validation and conflict detection in `src-tauri/src/hotkey/mod.rs`.

## Notes

1. Web fallback listeners are disabled in Tauri runtime path to avoid duplicate triggering (`src/hooks/useVoiceWave.ts`).
