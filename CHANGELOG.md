# Changelog

All notable changes to VoiceWave are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), versioned per [Semantic Versioning](https://semver.org/).

---

## [0.5.0] – 2026-07-04

The biggest VoiceWave release yet: a fresh install now transcribes offline out
of the box with no setup, an optional on-device AI polish pass, and dictation
that lands in every app — including terminal-style GUIs like Codex.

### Added
- **Bundled offline transcription engine.** The installer now ships a
  self-contained CPU faster-whisper runtime, so a fresh install transcribes the
  default model immediately — no Python, no manual setup, fully offline.
- **On-device AI polish (opt-in).** An optional local LLM pass cleans up filler,
  capitalization, and punctuation after dictation and offers the polished text
  on the pill. Runs entirely on-device, gated by a strict fidelity validator so
  it never changes your meaning, and is **off by default** (`llm_polish_enabled`).
- **Dictionary export / import.** Back up and move your custom-term dictionary
  between machines.
- **Interactive pill actions.** One-tap actions on pill notices (e.g. copy the
  transcript, add a term) via a typed action channel.
- **Mic-volume guard.** Detects a too-low input level and surfaces a
  Dynamic-Island-style pill notice so quiet-mic dictations don't silently fail.

### Changed
- **Terminals detected by process, not window title.** Dictation now pastes
  correctly into the Codex and Claude desktop apps (previously misread as CLIs
  by their window titles and forced to clipboard-only).
- **Quieter polish notifications.** The "Polished version ready" pill surfaces
  occasionally rather than after every dictation — it stays out of the way while
  fallback and error notices still show every time.
- Batch of performance and reliability refinements across the audio, inference,
  and insertion paths.

---

## [0.4.0] – 2026-06-14

### Added
- **In-app auto-update.** VoiceWave now checks GitHub on launch and offers a one-click "Install & Restart" when a newer signed release is available — no more manual re-download. Updates are cryptographically signed and verified before install.

---

## [0.3.1] – 2026-04-25

### Added
- Cold-start prewarm at app launch — first dictation drops from 2–5 s to ~500 ms
- Extra hallucination guard: `log_prob_threshold = -1.0` on primary decode alongside existing `no_speech` and `compression_ratio` floors
- Opt-in `whisper.cpp` models: `wcpp-small.en` (~466 MB) and `wcpp-large-v3-turbo` (~1.6 GB)
- Vulkan backend available behind the `whisper-vulkan` cargo feature
- `SendInput` now refuses to type into Windows security dialogs (UAC, Credential Manager, PIN prompts)
- winget package `VoiceWave.LocalCore` surfaced in README and hero copy

### Fixed
- Soft word endings ("s", "th", "f", drifted "e") no longer clipped — post-release capture window lifted to 300 ms
- Volume-adaptive trim thresholds — quiet speakers and post-pause resumptions now land fully
- Push-to-talk release no longer drops silently on key bounce
- Clipboard paste keeps dictated text — no more "old content pasted" after delayed Ctrl+V
- Worker stdout drained between requests to prevent ID-mismatch stalls after cancel/retry

### Changed
- All aggressive DSP (pre-emphasis, gain normalization, noise attenuation, hum notch, soft limiter) defaults to off — Whisper receives audio as captured
- Decode threads reserve one CPU core for the UI to stop taskbar stutter during transcription
- Download CTA and README installer link updated to v0.3.1 asset

---

## [0.3.0] – 2026-04-19

### Added
- `whisper.cpp` opt-in models and Vulkan backend (`whisper-vulkan` cargo feature)
- NSIS installer published to GitHub Releases
- `v0.3.0` release asset

### Fixed
- DSP steps defaulted off; tail padding extended to 300 ms
- Resampler aliasing killed; CPU precision raised
- Pill sync perfected; shadow box removed; hallucination suppression tightened
- Bassy cue sounds synced to pill show/hide
- `no-speech` rejection thresholds relaxed
- `where.exe` console flash suppressed on startup
- Real download progress shown for faster-whisper prefetch
- Worker timeout extended to 30 min for first-time model downloads
- VS cmake added to PATH in check and build scripts

### Changed
- Marketing site refreshed: real app logos in marquee, editorial section rhythm, "Out Now" particle text

---

## [0.2.2] – 2026-04-18

### Fixed
- Installer now ships with correct runtime DLLs
- Reliable transcription in installed (non-dev) builds

---

## [0.2.x] – 2026-02-10 to 2026-04-17

Early Windows baseline. Key milestones:

- Phase A CPU acceleration and latency sweep (2026-02-11)
- Phase B faster-whisper integration; CUDA backend selection with runtime readiness checks
- Floating pill runtime and custom hotkey cue audio
- Local entitlement billing and Pro command surfaces
- UI state flow stabilized: `idle → listening → transcribing → inserted/error`
- Mic monitor lifecycle hardened; browser insertion reliability improved
- Hotkey/settings/model safety hardened

---

[0.3.1]: https://github.com/rithwik1510/VoiceWave-Open-Source-WhisperFlow-Alternative/releases/tag/v0.3.1
[0.3.0]: https://github.com/rithwik1510/VoiceWave-Open-Source-WhisperFlow-Alternative/releases/tag/v0.3.0
[0.2.2]: https://github.com/rithwik1510/VoiceWave-Open-Source-WhisperFlow-Alternative/releases/tag/v0.2.2
