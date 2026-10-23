# Plan 022: telemetry and diagnostics (ships in 0.7.5)

Approved by the owner 2026-10-23. Goal: know whether installs are real people,
whether they stay, and what breaks for them; and turn silent failures into
useful GitHub issues. Ships with 0.7.5 so the Parakeet cohort is measured.

## Decisions

- **No re-prompt.** `ANALYTICS_CONSENT_VERSION` stays 1 (same call as Phase A).
  `ANONYMOUS_USAGE_DETAILS` is updated to list every event.
- **Enums only.** Every parameter is a value from a closed set defined in code.
  Never error messages, transcripts, paths, device names, or settings.
- **Same gate as today.** Nothing is sent unless usage counting is on and an
  app ID is configured. Rust never talks to the network for telemetry; it
  queues facts and the frontend sends them through `telemetry.ts`.
- **Panics, not "crashes".** The panic hook also fires for panics the app
  survives (caught in tokio tasks and `mic_keeper::guarded`), so the event is
  `App.panicked`, counted once per UTC day per area. A hard kill or power loss
  is not detectable reliably and is not counted.
- **Not in scope:** daily dictation counts and latency medians, a live payload
  viewer, re-prompting.

## Why activeDaily looks like "install day only"

`App.activeDaily` fires on the first successful dictation of each UTC day: it
means "dictated today", not "app ran today". Churn and "running but broken"
look the same in today's data. `App.launchedDaily` separates them.

## Contract (Rust ↔ frontend)

Both sides build against this. Names are exact.

### Telemetry queue

- Rust module `src-tauri/src/telemetry.rs`: a process-wide bounded queue
  (cap 64, drop oldest) of `TelemetryEvent`, plus `telemetry::record(event)`
  callable from any thread, sync or async, without an `AppHandle`. After the
  app handle exists, `record` also emits the nudge event below.
- Nudge event: `voicewave://telemetry-pending`, payload `()`/null.
- Command: `drain_telemetry() -> Vec<TelemetryEvent>`: returns and clears the
  in-memory queue **and** the panic file (below). The frontend calls it once
  after startup (covers events from before its listener existed, e.g. prewarm
  worker failures and panics from the previous run) and on every nudge.
- Serde: `#[serde(tag = "type", rename_all = "kebab-case")]`; enum values
  `rename_all = "kebab-case"`.

```rust
enum TelemetryEvent {
    DictationFailed { kind: DictationFailureKind },
    WorkerCrashed { worker: WorkerKind, exit: WorkerExit },
    Panicked { area: PanicArea },
}
enum DictationFailureKind {
    Setup,          // MissingModel
    MicMissing,     // AudioError::MissingInputDevice / DeviceNotFound
    MicOpen,        // Devices | DefaultInputConfig | BuildStream | PlayStream | RuntimeStream | UnsupportedSampleFormat
    EngineStart,    // speech engine failed to start / warm up
    Timeout,        // decode deadline missed (whether or not audio was kept)
    Transcription,  // any other decode failure
    EmptyResult,    // decode finished without a final transcript (post_process.rs:267)
    Insert,         // Insertion { success: false } (not elevation, which succeeds clipboard-only)
    NoSpeech,       // AudioError::NoSpeechDetected / low-confidence gate (Ok(None) paths)
    Other,
}
enum WorkerKind { AsrGpu, AsrCpu, FasterWhisper, Polish }
enum WorkerExit { Crash, Timeout, SpawnFailed }
enum PanicArea { State, Inference, Audio, Insertion, Hotkey, Models, Settings, Other, Dependency }
```

TS mirror (in `src/types/voicewave.ts`):

```ts
type DictationFailureKind = "setup" | "mic-missing" | "mic-open" | "engine-start" | "timeout"
  | "transcription" | "empty-result" | "insert" | "no-speech" | "other";
type WorkerKind = "asr-gpu" | "asr-cpu" | "faster-whisper" | "polish";
type WorkerExit = "crash" | "timeout" | "spawn-failed";
type PanicArea = "state" | "inference" | "audio" | "insertion" | "hotkey" | "models"
  | "settings" | "other" | "dependency";
type TelemetryEvent =
  | { type: "dictation-failed"; kind: DictationFailureKind }
  | { type: "worker-crashed"; worker: WorkerKind; exit: WorkerExit }
  | { type: "panicked"; area: PanicArea };
```

User cancel is never recorded. Stale sessions (replaced by a newer
dictation) are not recorded.

### Other contract changes

- `SupportContext` gains `launchedAtLogin: boolean` (true when this process
  was started with `--autostart`).
- `SpeechEngineStatus` gains `gpuVendor: "nvidia" | "amd" | "intel" | "other" | null`,
  from the PCI vendor id (0x10DE, 0x1002/0x1022, 0x8086), null with no GPU.
- New command `get_diagnostics_report() -> String`.
- `open_issue_report(engine)` keeps its signature and now also fills the
  issue form's `diagnostics` field with the report.

## Frontend events (`src/lib/telemetry.ts`)

| Event | Parameters | Dedupe |
|---|---|---|
| `App.launchedDaily` | `VoiceWave.App.launchMode`: `autostart` \| `manual` | once per UTC day; re-checked hourly while running |
| `Dictation.failed` | `VoiceWave.Dictation.failureKind` | once per UTC day per kind |
| `Speech.workerCrashed` | `VoiceWave.Worker.kind`, `VoiceWave.Worker.exit` | once per app session per kind+exit |
| `App.panicked` | `VoiceWave.Panic.area` | once per UTC day per area |

`describeFunnelEvent` returns a parameter map instead of a single parameter.

**Context on every signal** (existing events too), next to version and channel:

| Key | Values | From |
|---|---|---|
| `VoiceWave.Speech.path` | `gpu` \| `cpu` \| `preparing` \| `none` | `speechEngine.runningOn` / `preparing` |
| `VoiceWave.Speech.modelFamily` | `parakeet` \| `turbo` \| `small` \| `other` \| `none` | `settings.activeModel` |
| `VoiceWave.Gpu.vendor` | `nvidia` \| `amd` \| `intel` \| `other` \| `none` | `speechEngine.gpuVendor` |
| `VoiceWave.Os.major` | `10` \| `11` \| `other` | `supportContext.osVersion` |

Context comes from a provider registered by `useVoiceWave`, read at send
time. It is injected through `AnonymousUsageDependencies` so existing tests
keep passing (default provider returns no extra keys until registered).

## Diagnostics report (`get_diagnostics_report`)

Plain text, no audio or dictated text, capped at ~6 KB. First line: "Check
before posting: this includes your microphone's name and Windows version.
Never audio or anything you dictated."

- VoiceWave version, Windows version, CPU support, launched at login.
- Speech: active model, model installed yes/no, running on GPU/CPU/preparing,
  GPU name, GPU qualification outcome.
- Microphone: configured device (or "system default"); hotkey.
- Recent problems: the last 20 WARN/ERROR **events** from the newest log file,
  first line of each only (no backtraces), each capped at 300 chars, with
  the user profile directory replaced by `%USERPROFILE%` (move
  `redact_profile_dir` from `asr_worker.rs` into `support.rs` and reuse it).

## UI

- Settings → Diagnostics: "Copy diagnostics" button (shows "Copied") and a
  collapsible preview of the same text, in the existing Report a problem row
  area. Light and dark.
- Report a problem: unchanged button; the issue now arrives with the report
  in the `diagnostics` field. The URL stays under 7,500 characters by
  dropping the oldest log lines first.
- `bug_report.yml`: new `diagnostics` textarea (`render: text`), label
  "Diagnostics (filled in by the app)"; attaching log files stays optional.

## Owner side (TelemetryDeck)

- Funnel: `App.installActivated` → `Onboarding.completed` →
  `Dictation.firstSuccess` → `App.activeDaily` on a later day.
- `App.launchedDaily` vs `App.activeDaily`, users per day: the gap is
  "running but not dictating".
- `Dictation.failed` by `failureKind`; `Speech.workerCrashed` by kind and
  exit; `App.panicked` by area. Split any of them by `Gpu.vendor`,
  `Speech.path` or `modelFamily`.
- Version breakdown for 0.7.5 adoption and Parakeet share.
