# Next Release Asset 0.2.5

## Why this is needed

The current website-downloaded installer can still show the hotkey bug where pressing `Ctrl+Windows` starts dictation and then it stops immediately, sometimes with a terminal window flashing open and closed.

This is **not** because of the laptop.

This happens because the installed app package is missing the faster-whisper worker/runtime path expected by the default `fw-small.en` backend. The hotkey itself is not the problem. The helper process used for dictation is the part that was failing in the installed build.

## Fix already prepared in code

The next release asset should include these changes:

1. Bundle `faster-whisper/worker.py` inside the Windows installer resources.
2. Copy that worker into `src-tauri/windows/faster-whisper/worker.py` during bundle prep.
3. Resolve the faster-whisper worker from installed app resource paths, not only repo/dev paths.
4. Resolve Python more safely for installed builds.
5. Start the worker without opening a visible terminal window on Windows.
6. Show beta-user-friendly runtime errors instead of dev-only script instructions.

## Files involved

- `src-tauri/tauri.conf.json`
- `scripts/tauri/prepare-tauri-bundle-windows.ps1`
- `src-tauri/src/inference/faster_whisper.rs`
- `src-tauri/src/state.rs`

## What to do when releasing 0.2.5

1. Build a fresh Windows installer from the patched codebase.
2. Publish a real GitHub release asset for `v0.2.5`.
3. Update the Render website download env var to the new direct installer URL.
4. Re-download from the website button and test on the installed app, not just local dev.

## Expected result after 0.2.5 release

- Pressing `Ctrl+Windows` should start dictation normally.
- No terminal/console flash should appear.
- Dictation should not instantly stop because of missing packaged worker/runtime files.
- The website installer should behave the same as the locally tested build.

## Suggested release link format

Use this pattern once the release asset exists:

`https://github.com/rithwik1510/VoiceWave-Open-Source-WhisperFlow-Alternative/releases/download/v0.2.5/VoiceWave.Local.Core_0.2.5_x64-setup.exe`

## Quick reminder

Do not point the website to `0.2.5` until the actual GitHub release asset is published, otherwise the download button will lead to a 404 page.
