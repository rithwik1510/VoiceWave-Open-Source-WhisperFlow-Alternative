# Changelog

All notable changes to VoiceWave are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), versioned per [Semantic Versioning](https://semver.org/).

---

## [Unreleased]

A smaller installer, no more Pro screens, an easy way to report problems, and
dictation that lands in terminals and survives a failed transcription.

### Added

- **Dictation into terminals.** Windows Terminal, PowerShell, Command Prompt,
  Git Bash and other terminals now get the text pasted at the prompt instead
  of only copied. Line breaks become spaces there, so dictated text can never
  run a command on its own.
- **Say "press enter" to send.** End a dictation with "press enter" (or "hit
  enter") as its own sentence and VoiceWave presses Enter after the text
  lands. Say it on its own to press Enter only. It follows the Spoken edit
  commands switch.
- **Try again after a failed transcription.** When transcription fails or
  times out, the pill offers Try again, which transcribes the same recording.
  The audio stays in memory only, and only until your next dictation.
- **Report a problem.** Help → Report a problem opens a pre-filled GitHub
  issue with your app version, Windows version and speech engine. Nothing is
  sent until you submit it on GitHub. Open logs folder shows the log files to
  attach.
- **Log files.** VoiceWave now keeps a week of daily log files, including a
  record of any crash. Logs never contain what you dictated.
- **Setup and reliability counts** in the anonymous usage count: which setup
  step was reached, whether setup finished or was skipped, whether the first
  dictation ran on the GPU or the CPU, failed model downloads, and GPU-to-CPU
  fallbacks. Each carries one category and nothing else. The existing switch
  in setup and Settings → Diagnostics turns all of it off.

### Changed

- **Smaller installer.** The bundled Python runtime no longer ships debug
  symbols, build libraries, packaging tools, test suites, an unused voice
  detection library, or duplicate AI polish libraries.
- **Cleaner upgrades.** Installing a new version now removes the old speech
  runtime first, so files from earlier versions no longer pile up.
- **Polish profiles moved to Settings → Polish.** Standard and Literal work
  without the AI model; Coding and Writing use it.
- **Everything is free, so the Pro screens are gone.** The Pro and Pro Tools
  pages, release-offer cards and Pro badges were removed.
- The sign-in screen and sync messages are hidden. Release builds have no
  cloud accounts, and the old sign-in accepted any email without doing
  anything.
- **Hands-free waits while you think.** Hands-free ends when you press the
  shortcut again (Escape cancels). It only stops on its own after 30 seconds
  of silence or at 5 minutes, and long recordings get more time to
  transcribe.
- **Apps running as administrator.** Windows blocks typing into them from a
  normal app, so VoiceWave now copies the text and says so, instead of
  reporting it as inserted.

---

## [0.7.0] – 2026-10-17

GPU dictation on any vendor's graphics card, a model picked for your PC, and
a more reliable first run.

### Added

- **GPU dictation through whisper.cpp and Vulkan.** PCs with a discrete GPU
  from NVIDIA, AMD or Intel (Vulkan 1.2) now transcribe on the GPU. There is
  no CUDA or driver download. The speech engine runs in its own worker
  process, so a misbehaving graphics driver cannot take the app down with it.
  faster-whisper stays the engine for the CPU and for PCs with only
  integrated graphics.
- **One-time graphics card check.** The first time the GPU engine starts,
  VoiceWave runs a few short test clips to confirm the card produces correct
  text at a useful speed. Until it passes, dictation keeps using the CPU. On
  NVIDIA cards the first run can take about 25 to 40 seconds while shaders
  compile.
- **A model picked for your PC.** Setup and the Models page show two models,
  Small and Large Turbo, matched to your hardware and to the engine your PC
  will use. Existing faster-whisper users who can use the GPU get a one-time
  offer to switch, which downloads, checks and switches the model, and
  resumes after a restart.
- **Status and control.** The Models page says "Running on <your GPU>" or
  "Running on your processor". Settings → Dictation has a GPU acceleration
  switch (Auto or Off). Installed models you are not using can be removed.
- **Automatic step-down to the CPU.** If the GPU stalls or errors, the same
  audio is transcribed on the CPU and inserted once, and the pill says so
  once per session. Two failures in a row pause the GPU for five minutes. A
  driver that fails across two sessions is turned off for 7 days, or until
  you update the app.

### Changed

- **Verified model downloads.** The two whisper.cpp models (Small and Large
  Turbo) are checked against a pinned SHA-256 hash while they download. A
  partial download is verified before it replaces anything, so a working
  model is never deleted before its replacement is ready, and downloads
  resume by default. faster-whisper models and the AI polish model are
  still checked for completeness and size only.
- Faster recovery from a hung GPU: a stuck GPU is detected in seconds
  instead of costing one dictation about a minute.
- AI polish is turned off, with a plain explanation, on CPUs without AVX2.
  Processors without SSE4.1 are told in setup that this PC isn't supported.
- Removed the in-process whisper-rs engine and the unused CUDA add-on
  download. GPU dictation is the Vulkan worker now.

### Fixed

- **Clipped word starts.** Audio trimming cut soft first sounds ("Restart"
  became "Please start"). It now keeps 500 ms of padding at both ends. On a
  scripted 20-phrase test with a real voice, exact matches went from 9 to 12.
- Dictionary terms no longer force their capitalization everywhere. "Things"
  in your dictionary no longer capitalizes every "things". Only terms whose
  spelling carries casing (multi-word names, inner capitals, digits, dashes)
  are enforced.
- A longer retry transcript no longer replaces a better one just for being
  longer.
- **First-run setup.** You can always continue past the practice step while
  the model downloads, and the download keeps running. The model screen
  shows live progress (percent, megabytes, Cancel, Retry or Resume), follows
  a download already running, and defaults to the model recommended for this
  PC. Microphone failures now say what is wrong and link to the Windows
  microphone privacy settings.
- Dictating with no model installed fails when you press the hotkey, not
  after you finish speaking, and the message brings the VoiceWave window
  forward. Common setup problems (no mic, mic blocked or busy) get plain
  messages.
- A second download request for a model that is already downloading shows its
  progress instead of starting a second copy, and Cancel stops only that
  model's download.
- Model files left behind by older versions are adopted at startup, so
  upgraders are not sent back through setup.
- A graphics driver that hangs while listing devices can no longer block the
  model list and setup screen forever.
- New GPU users are kept off slow paths on first run: early GPU decodes get
  extra time while the driver's shader cache warms up, and a dictation during
  the one-time check waits for it instead of loading the model on the CPU.

---

## [0.6.1] – 2026-10-15

First-run reliability, faster first dictation, and safer storage.

### Fixed

- **Model download progress no longer sits at 0%.** The speech-model download
  now grows the progress bar steadily instead of jumping from almost nothing
  to done at the very end, and Cancel keeps what was fetched so Install
  genuinely resumes.
- **No hidden background download on first launch.** VoiceWave no longer
  starts downloading the default model before you choose one, and it no
  longer contacts Hugging Face on every launch.
- **First dictation after launch is much faster.** Startup could load the
  speech model twice, which made the first dictation take 10–20 seconds. It
  now loads once, and if the model is still loading when you finish speaking,
  the pill says so.
- Dictating before the model has finished downloading now explains that the
  model isn't ready yet, instead of waiting about 45 seconds and failing.
- If text could not be inserted or copied, the pill now shows an error
  instead of "Inserted".
- The pill shows Inserted as soon as the text lands, even if saving History
  is slow.
- Focused-app detection uses the process name, so window titles such as
  "Knowledge base" are no longer mistaken for Microsoft Edge.
- Emoji are typed as a single unit and can no longer be split by other input.
- A briefly locked encryption key file (for example during an antivirus scan)
  can no longer cause your history, dictionary, or snippets to be reset. An
  existing key is never overwritten.
- History now keeps up to 1,000 recent dictations under every retention
  setting (previously 200), and the Settings page says so. Starred entries
  export with a ★.

### Changed

- **Model recommendation per PC.** Setup and the Models page recommend the
  speech model that suits your hardware and note when Large Turbo will be
  slow on this PC.
- Installed models are checked for completeness (all model files present).
  The previous "checksum verified" message was inaccurate and has been
  removed.
- Lower memory use: the AI polish worker reserves about 450 MB less memory,
  and the speech worker no longer loads unused video codecs.

- **Anonymous usage count is now on by default for new installs.** First-run
  setup shows what is counted, with a switch to turn it off before the first
  dictation. Existing users are asked once (Keep on / Turn off); a stored
  "off" is never flipped without that choice, and users who had already
  turned it on are not asked again.
- What is sent is unchanged: a random installation identifier, app version,
  GitHub distribution channel, one activation marker, and at most one active
  marker per day. Audio, transcripts, account identity, filenames, settings,
  and device details are never sent. It can be turned off anytime in
  Settings → Diagnostics.

---

## [0.6.0] – 2026-08-12

Hands-free dictation, full dark mode, and deeper usage insights.

### Added

- **Hands-free dictation.** Double-tap the push-to-talk shortcut to keep
  listening without holding the keys. Pause when finished or press the
  shortcut again; Escape cancels the active dictation.
- Hands-free capture ends automatically after the configured silence period
  while retaining the release tail when stopped manually, so quiet final
  syllables are not clipped.
- **Complete light, dark, and system theme support.** The application shell,
  dashboard, floating pill, settings, and supporting surfaces now share the
  same dark design language and follow the selected Windows theme without a
  startup flash.
- **Deeper statistics.** Insertion-success-aware totals, a GitHub-style streak
  heatmap, and insight panels make activity and reliability trends easier to
  understand.

### Changed

- The recording pill and dashboard now expose the active hands-free state and
  explain both pause-to-finish and shortcut-to-finish behavior.
- Theme controls are available in Settings with Light, Dark, and System
  choices.

## [0.5.9] – 2026-07-31

Usage visibility and small dashboard-polish release.

### Added

- **Optional anonymous activated-install counting.** Users can explicitly enable
  a content-free usage count in Settings. VoiceWave then records one activation
  after the first successful dictation and at most one active marker per day.
  Updates retain the same random installation identifier, so updating does not
  create another user.
- Analytics events contain only the random installation identifier, app
  version, and GitHub distribution channel. Audio, transcripts, account
  identity, filenames, settings, and device details are never sent.

### Fixed

- The Home greeting no longer contains a hardcoded name. Signed-in users see
  their profile name; guest users see a general VoiceWave welcome.
- The website release counter no longer adds updater metadata checks or demo
  assets to its total. It now reports and labels only installer transfers.

---

## [0.5.8] – 2026-07-28

Crash-safety release. VoiceWave could be left unable to start by something as
ordinary as losing power at the wrong moment; it no longer can.

### Fixed

- **VoiceWave could refuse to launch after an unclean shutdown.** If the machine
  lost power, blue-screened, or the app was force-killed while it happened to be
  saving one of its data files, that file could be left with its size recorded
  but none of its contents written. On the next launch VoiceWave would fail to
  start — no window, no message, nothing to act on — and the only way out was
  knowing which file to delete by hand. Every store now recovers instead:
  it finishes the interrupted save, falls back to the backup copy, and if
  neither can be read, sets that one file aside and starts from defaults. The
  app launches.
- **Saves are now crash-safe.** Data is written to a temporary file and flushed
  all the way to disk before it replaces the original, with the previous copy
  kept until the new one is durable. Losing power mid-save can no longer destroy
  your settings, dictionary, snippets, history, stats, or diagnostics.
- **A damaged encryption key no longer blocks startup.** It is set aside and
  regenerated. The store it protected resets to defaults, which is a local cache
  loss rather than an app that will not open.

### Changed

- If anything did have to be reset, VoiceWave now says so once on the next
  launch rather than quietly behaving differently.
- A leftover plaintext diagnostics backup from an older migration no longer uses
  a filename that collides with the new backup, and stale backups are removed
  once the main file is confirmed readable.

---

## [0.5.7] – 2026-07-28

First-run reliability release. Everything here is about the very first minutes
after install — the one-time speech-model download and the setup flow around it.

### Fixed

- **A failed model download no longer traps setup.** If the one-time speech
  model download failed — no connection, a captive hotel or office network, a
  dropped link — setup showed a progress bar that never moved, with no error
  message and no way forward. VoiceWave now tells you what happened and offers
  Retry, and whatever downloaded before the failure is kept.
- **Cancel actually cancels a model download.** The Cancel button had no effect
  on the download running underneath it, which kept going regardless. It now
  stops the download and keeps the bytes already fetched, so pressing Install
  again resumes instead of starting over. Pause and Resume, which never applied
  to these models, are no longer shown.
- **Quitting during a download no longer leaves a process behind.** VoiceWave
  now shuts down its speech and polish helpers on exit, instead of leaving one
  downloading invisibly after the app was closed.

### Changed

- Setup shows how much of the model has arrived ("213 MB of 465 MB") rather than
  a bare percentage, so a slow connection reads as slow instead of stuck.
- The welcome screen no longer promises a fixed setup time it cannot guarantee
  on a slow connection.

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
