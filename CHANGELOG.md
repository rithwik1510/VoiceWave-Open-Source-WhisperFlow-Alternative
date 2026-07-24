# Changelog

All notable changes to VoiceWave are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), versioned per [Semantic Versioning](https://semver.org/).

---

## [0.5.6] – 2026-07-24

### Added

- **Voice snippets.** Say a phrase you choose — "my support reply", "my work
  address" — and VoiceWave inserts your saved text exactly as you wrote it.
  Casing, punctuation, line breaks, URLs, and indentation are preserved
  byte-for-byte. Triggers work on their own or inline inside a longer
  dictation, and the same trigger can expand more than once in one utterance.
  Snippets have their own page in the app with search, edit, and delete.
- **Snippet expansions are never rewritten.** Saved text is protected through
  deterministic formatting and the on-device AI polish pass, so neither the
  formatter nor the language model can reword content you own. If a polish
  result would disturb a protected expansion, VoiceWave discards it and falls
  back to the deterministic result.
- Snippets are stored encrypted on your device, work signed out and offline,
  and replicate across your devices when you are signed in. Limits: 250
  snippets, 60-character triggers, 4,000-character expansions, 16 expansions
  per dictation.

### Fixed

- **Removed a plaintext dictionary backup file** that an older migration left
  behind on disk. Your dictionary is encrypted at rest, and that stray copy no
  longer exists.
- **Unapproved dictionary suggestions no longer influence transcription.**
  Terms waiting in the review queue were being fed to the recognizer before you
  approved them. Only approved terms are used now.
- The on-device polish pass no longer alters or drops recognized proper nouns
  such as person, place, and company names.
- Local polish CPU use is bounded to a four-thread ceiling. This was documented
  under 0.5.5 but landed after that build was cut; it ships here.

### Changed

- **Dictionary storage was rebuilt to be local-first.** The encrypted on-device
  dictionary is now the single source of truth in every account state, with
  optional cloud storage acting purely as replication on top of it — including
  tombstoned deletes that propagate instead of resurrecting, and local writes
  that never roll back when a remote write fails.

  This is groundwork, not a change you will notice today: cloud sync ships
  disabled in release builds, so every install already used the local
  dictionary. It removes a latent split-brain design that would have caused
  dictionary edits to silently miss transcription once sync is switched on.

---

## [0.5.5] – 2026-07-11

### Added

- **Mode-aware polish profiles.** Standard, Coding, Writing, and Literal now
  have explicit delivery policies. Coding and Writing use profile-specific
  on-device prompts and insert only Rust-validated output; Literal never calls
  the language model.
- **Profile provenance.** History records the selected profile, inserted text,
  accepted polish, latency, and fallback outcome.

### Changed

- Selecting Coding or Writing automatically enables and prepares the local AI
  polish engine. The persisted profile is warmed after launch and after the
  one-time model download, while deterministic formatting remains available.
- _(Correction: the four-thread polish CPU ceiling was documented here in
  error. It was committed after the 0.5.5 build was cut and actually shipped in
  0.5.6.)_

---

## [0.5.2] – 2026-07-10

### Fixed

- **Event-driven global hotkeys.** Replaced the polling `GetAsyncKeyState` monitor with a Windows low-level keyboard hook. Push-to-talk release is now instant — no debounce delay — and auto-repeat/bounce no longer produce duplicate edges.
- **Resilient cue audio.** Rewrote the cue system with worker health tracking and device fingerprinting, so it survives output device switches mid-session and recovers gracefully from transient audio failures.
- **Pill appears instantly on Listening.** The floating pill is now fully re-established (size, position, click-through, visibility) before mic device setup begins, so users see feedback immediately on hotkey press.
- **Dictation session race eliminated.** The capture/decode flow runs in its own task, letting the event-driven hotkey dispatcher process a rapid push-to-talk release without the old start/release task race.

---

## [0.5.1] – 2026-07-05

### Added

- **Check for updates in Settings.** New Settings → Updates pane shows the
  installed version and a "Check now" button, so you can see you're up to date
  (or install a new version) on demand instead of waiting for the silent
  launch-time check.

### Fixed

- The AI-polish worker's GPU probe no longer opens a console window on setups
  running a GPU-capable build.

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
