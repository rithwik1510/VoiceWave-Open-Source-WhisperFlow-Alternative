# VoiceWave Phase I Implementation Notes

## Phase Tag

`Phase I - Core Audio and Inference Foundation`

## Platform Scope Override

1. Effective date: 2026-02-10.
2. Active implementation/validation scope is Windows-only.
3. macOS validation is deferred until macOS hardware is available.

## Status (2026-02-11)

`Implemented and revalidated for Windows rescue baseline`

Phase I desktop dictation uses real whisper.cpp runtime decoding. Fixture mode is retained only for deterministic test/harness flows.

## Implemented Core

1. Audio capture + VAD segmentation:
   - `src-tauri/src/audio/mod.rs`
2. Runtime dictation flow + state/events:
   - `src-tauri/src/state.rs`
   - `src-tauri/src/lib.rs`
3. Whisper runtime decode (faster-whisper / whisper.cpp speech workers):
   - `src-tauri/src/inference/mod.rs`
   - `src-tauri/src/inference/asr_worker.rs`
4. Settings persistence + safe VAD bounds:
   - `src-tauri/src/settings/mod.rs`
   - `src-tauri/src/state.rs`
5. Transcript sanitization before insertion/history/dictionary:
   - `src-tauri/src/transcript/mod.rs`
   - `src-tauri/src/state.rs`

## Rescue Hardening Added

1. Bracketed non-user artifacts such as `[BLANK_AUDIO]` are stripped before user-facing ingestion.
2. VAD threshold is clamped to a safe range and includes a recommended reset path.
3. Frontend warns on likely low-quality microphone profiles (for example Bluetooth hands-free paths) and provides recovery actions.
4. Local-state backup/reset support script added for contamination recovery:
   - `scripts/support/backup-reset-local-state.ps1`
   - `docs/troubleshooting/local-state-recovery.md`

## Command/Event Contract (Phase I Runtime)

1. Commands:
   - `get_voicewave_snapshot`
   - `load_settings`
   - `update_settings`
   - `start_dictation(mode)` where `mode` is `microphone | fixture`
   - `cancel_dictation`
2. Events:
   - `voicewave://state` -> `{ state, message? }`
   - `voicewave://transcript` -> `{ text, isFinal, elapsedMs }`

## Validation Evidence (2026-02-11)

1. `npm run test -- --run`
   - Result: pass (`3` files, `7` tests).
2. `npm run build`
   - Result: pass (`built in 2.87s`).
3. `npm run phase3:validate`
   - Result: pass, includes Rust desktop-feature compile path with no-space Windows strategy.
4. Added Rust coverage for transcript sanitization:
   - `src-tauri/tests/transcript_sanitization.rs`

## Remaining Non-Blocking Gates

1. Phase battery/thermal signoff still requires a >= 30.0 minute artifact.
2. Manual workflow acceptance on target machine is still required for release confidence:
   - Notepad
   - VS Code
   - Browser text field/editor
