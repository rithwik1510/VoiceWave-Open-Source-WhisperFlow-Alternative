# Plan 024: Usage analytics on PostHog, diagnostics and problem reports (0.7.7)

Status: **draft v2, rewritten 2026-10-28 after owner decisions. Not yet approved for implementation.**
Branch `release/0.7.7`, cut from main at the 0.7.6 merge (`9a214a1`). Every path and line number below was checked
against that commit unless it says otherwise.

v1 of this plan (owner's own spec, 2026-10-28, kept verbatim in
`plans/artifacts/telemetry-2026-10/024-v1-telemetrydeck-spec.md`) kept TelemetryDeck and added events on top. v2 moves
analytics to PostHog and keeps v1's install source, consent, support file and UTM work. What changed, and why, is in
section 1.

## 0. Goal and decisions

**Goal.** Answer three questions with data we trust:

1. **How many users do we really have?** Real people, not scanners, sandboxes or our own dev builds.
2. **Is each user real, and are they still here?** One person's history should be readable: installed, set up,
   dictated on these days, then went quiet.
3. **How does using VoiceWave go for them?** Does dictation work, how fast is it, what fails, did updates install,
   did it crash?

**Owner decisions (2026-10-28):**

| # | Decision |
|---|---|
| 1 | Move analytics to **PostHog**, for per-person timelines, cohorts and retention |
| 2 | **No event per dictation.** Look at each user's experience as a whole: one summary per active day, plus events for milestones and problems |
| 3 | Rewrite this plan around that, including anything else that makes it better |
| 4 | Carried from v1: optional "where did you hear about us" question (fixed choices); install source from an explicit installer marker only; consent prompt v2, shown once; values only from fixed lists |

**Privacy line (unchanged, non-negotiable).** Never sent: audio, transcript text, dictionary or snippet content,
window titles, app or exe names, file paths, device names, error message text, or settings values outside the
fixed lists below. VoiceWave's promise is that your words never leave your PC; analytics describes how the app
behaves, never what you said.

## 1. What changes from v1, and why

| v1 | v2 | Why |
|---|---|---|
| TelemetryDeck, sent from the webview | **PostHog, sent from Rust** | PostHog has person timelines, cohorts, retention and funnels. Most events start in Rust (dictation, workers, downloads, insertion), so sending from Rust removes the drain-to-TS hop that loses events today |
| Outbox in localStorage | **Outbox file in Rust** | Survives the webview, and works while the app sits hidden in the tray for days |
| Two consent tiers (basic, reliability) | **One switch**, consent version 2 | The event set is now small, and one switch is easier to explain and to build |
| Many once-per-day dedupe markers, `Event.delayed` | **Real timestamps** | PostHog accepts the time an event happened, so a late send still lands on the right day |
| `Insertion.targetDaily`, `Dictation.dailySummary` as separate ideas | **One `day_summary`** per active day | It carries the day's counts, speeds, failures and kinds of app in one event |
| `App.uncleanExit`, update not applied | **Properties of `app_launched`** | The next launch is where both are discovered |

TelemetryDeck stays **unchanged** in 0.7.7, sending what it sends today. That keeps the old charts running while
PostHog fills up. It's removed in the first 0.8.x release after PostHog is confirmed working. No new events are
added to it.

## 2. Privacy rules (apply to every section)

1. **Allowed property values:** a string from a fixed list in code (a Rust enum with `serde(rename_all =
   "snake_case")`), a number (count, milliseconds, bytes), a boolean, or the app version (must match
   `^\d+\.\d+\.\d+$`).
   - No other strings are ever sent.
   - A field we can't map goes out as `other`.
2. **App names become a kind on the PC.** The exe name is mapped to a kind of app (browser, code editor, and so on)
   inside the Rust function that reads it, and the raw name is dropped there.
3. **Error text becomes a kind on the PC.** It's never stored in the outbox.
4. **Anonymous identity only.** One random install id. No accounts, no hardware ids, no machine name.
5. **No IP and no location.**
   - The PostHog project is set to discard client IP data (section 3.6).
   - Every event carries `$geoip_disable: true`.
6. **Dev builds never send** unless a dev key is set on purpose (section 3.5).
7. **The consent check runs twice:** when an event is queued and again when it's sent. Turning the switch off
   empties the queue at once.
8. **The support file stays local.** It's written to disk only when the user asks, and VoiceWave never uploads it.

## 3. Analytics core (Rust)

### 3.1 Module

New `src-tauri/src/analytics/`:

| File | Job |
|---|---|
| `mod.rs` | Public API: `init`, `capture`, `set_context`, `set_consent`, `track` command |
| `events.rs` | `AnalyticsEvent`, the single enum of every event and its fields (section 4) |
| `outbox.rs` | The persisted queue: load, append, prune, pick due entries, remove |
| `posthog.rs` | Builds the `/batch/` body and sends it |
| `identity.rs` | Install id, session id, first-seen facts (registry) |
| `context.rs` | Base properties and the hypervisor check |

Existing `src-tauri/src/telemetry.rs` stays as it is for TelemetryDeck. Its `record()` also calls
`analytics::capture` with the matching `AnalyticsEvent`, so each failure is recorded once and reaches both.

### 3.2 Identity

**`install_id`:** a random UUID v4 (`rand` is already a dependency), used as PostHog's `distinct_id`.
- Stored as `InstallId` in `HKCU\Software\VoiceWave\LocalCore`, the same key as the install source (section 6).
- Created on the first launch of 0.7.7 or later, and never changed.
- It survives updates and reinstalls. It's deleted only by an uninstall with "Delete app data" ticked.
- It's deliberately **not** the TelemetryDeck id from localStorage. PostHog starts clean, and the two systems never
  need joining.

**`session_id`:** a new random UUID v4 on each launch, sent with every event.

**First seen:** `FirstSeenUtcMs`, `FirstSeenEstimated` and `FirstSeenVersion` in the same key, as in v1:
- When `settings.json` already existed at launch, this is a pre-0.7.7 install. Use that file's creation time and set
  `FirstSeenEstimated = 1`.
- Capture `settings_existed_at_launch` in a `OnceLock` at the very start of setup, before anything saves settings.
- `is_new_install = FirstSeenEstimated == 0`.

### 3.3 Base properties on every event

| Property | Values | Source |
|---|---|---|
| `app_version` | semver | `CARGO_PKG_VERSION` |
| `session_id` | UUID | 3.2 |
| `os_major` | `10`, `11`, `other` | `support::os_version()` |
| `gpu_vendor` | `nvidia`, `amd`, `intel`, `none`, `other` | speech engine status |
| `speech_path` | `gpu`, `cpu`, `preparing`, `none` | speech engine status |
| `model_family` | `parakeet`, `whisper_cpp`, `faster_whisper`, `none` | active model |
| `install_source` | `winget`, `scoop`, `direct`, `unknown` | section 6 |
| `install_age` | days since first seen (number) | 3.2 |
| `hypervisor` | `none`, `microsoft`, `vmware`, `virtualbox`, `kvm`, `xen`, `parallels`, `other` | 3.4 |
| `$lib` | `voicewave` | fixed |
| `$geoip_disable` | `true` | fixed |

**`context.rs`** holds the latest values behind a `Mutex`:
- Rust updates them wherever it already learns them: speech engine status changes (`state/gpu_engine.rs`), active
  model changes, and settings load.
- Base properties are resolved **when an event is queued**, so each one describes the app at that moment.

**Person properties.** `app_launched` and `consent_answered` also carry:
- `$set`: `app_version`, `os_major`, `gpu_vendor`, `speech_path`, `model_family`, `install_source`, `hypervisor`.
- `$set_once`: `first_version` and `first_install_source`.

That lets a PostHog person show their current setup at a glance.

### 3.4 Hypervisor check (sandbox and VM filter)

`context.rs` reads CPUID leaf 1, ECX bit 31 (a hypervisor is present). If it's set, it reads the vendor string from
leaf `0x4000_0000` through `core::arch::x86_64::__cpuid`, with no new crate:

| Vendor string | Value |
|---|---|
| (bit 31 clear) | `none` |
| `Microsoft Hv` | `microsoft` |
| `VMwareVMware` | `vmware` |
| `VBoxVBoxVBox` | `virtualbox` |
| `KVMKVMKVM` | `kvm` |
| `XenVMMXenVMM` | `xen` |
| ` lrpepyh  vr` | `parallels` |
| anything else | `other` |

**`microsoft` is ambiguous.** Ordinary Windows 11 PCs with Memory Integrity or WSL2 report it on bare metal. So it's
never used alone to call an install fake. The real-user rule (section 9) relies on whether the person dictated.
`vmware`, `virtualbox`, `kvm`, `xen`, `parallels` and `other` mark an install as a VM.

### 3.5 Keys and hosts

- **Release builds:** `option_env!("VOICEWAVE_POSTHOG_KEY")`, set by `release.yml` from the repo secret
  `POSTHOG_PROJECT_KEY`. The host is fixed at `https://eu.i.posthog.com` (EU Cloud, chosen 2026-10-28).
  - A PostHog project key (`phc_…`) can only write events, so it's safe inside the binary. That is how PostHog is
    designed.
- **Debug builds** (`cfg(debug_assertions)`) send nothing unless the runtime variable `VOICEWAVE_POSTHOG_DEV=1` is
  set.
  - When they do send, they use the same project, and every event carries `dev_build: true`.
  - The free plan allows one project, so there is no separate dev project. The project's "filter out internal and
    test users" setting excludes events that have `dev_build`, and it's on by default in every chart.
- **No key** (local release builds without the env var): `capture` does nothing and nothing is queued.

### 3.6 PostHog project setup (done 2026-10-28)

- **Organization and project:** EU Cloud organization "Rishi", project `VoiceWave` (id 324038), signed in as
  posanirithwik@gmail.com.
- **IP addresses:** project setting `anonymize_ips = true` (IP data discarded).
- **Turned off** (all web-only): session recording, autocapture, console log capture and performance capture.
- **Test account filter:** `dev_build` "is not set", applied by default.
- **Key:** the project key is stored as repo secret `POSTHOG_PROJECT_KEY` (2026-10-28).
- **Remaining:** build the dashboard in section 9 once real events arrive.

### 3.7 Outbox

**File:** `<data_dir>/analytics-outbox.json`, written with `atomic_file::atomic_write`.

```rust
struct OutboxEntry {
    uuid: String,          // UUID v4, PostHog's event uuid (dedupes a resend of the same entry)
    event: &'static str,   // e.g. "day_summary"
    timestamp: String,     // RFC 3339, when it happened
    properties: serde_json::Map<String, Value>, // base + event fields, resolved at enqueue
    attempts: u32,
    next_at_ms: u64,
}
```

**Rules:**
- **Caps:** at most 500 entries. When it's full, drop the oldest. Entries older than 14 days are dropped at load and
  before each send.
- **Flush loop:** one background thread (`std::thread`, no tokio needed). It wakes when an event is queued (debounced
  2 s), every 60 s while the outbox isn't empty, and once at startup. It sends due entries oldest first, up to 50 per
  POST.
- **Send:**
  - `POST {host}/batch/` with `{"api_key": KEY, "batch": [{event, distinct_id, properties, timestamp, uuid}]}`.
  - Use a small ureq agent with a 10 s connect and read timeout. Move `model_manager::net::http_agent`'s TLS setup
    into a shared `net` module and reuse it, so analytics gets the Windows certificate store fix from 0.7.6 too.
- **Responses:**

  | Response | Action |
  |---|---|
  | 2xx | Remove the batch's entries |
  | 400, 401, 403, 413 | Remove them (they can never succeed) and `warn!` once |
  | Network error, 408, 429, 5xx | `attempts += 1`; `next_at = now + min(2 s × 2^attempts, 5 min)` ± 20% jitter; stop this flush |

- **Quit:** on `RunEvent::Exit`, one best-effort flush with a 2 s total budget. Whatever's left waits for the next
  launch.

### 3.8 Consent gate

| State | Behaviour |
|---|---|
| `anonymous_usage_opt_in = false` | Nothing is queued; the outbox file is deleted |
| On, `analytics_consent_version >= 2` | Queue and send |
| On, version 1 (existing user who hasn't answered the v2 prompt yet) | Queue but **don't send**. If the answer is "Keep sharing", the queue goes out with its real timestamps. If "Turn off", it's deleted |
| On, version 1, prompt unanswered 3 times | Stop asking. PostHog stays off and the queue is deleted. TelemetryDeck carries on exactly as under v1 |

- `analytics::set_consent(opt_in, version, prompt_exhausted)` is called after settings load and whenever settings are
  saved, from the same Rust path that persists them.
- The settings save path purges the outbox when the switch turns off.

### 3.9 Frontend events

Some events start in the webview: onboarding steps, consent answers, the acquisition answer, feedback, and update
failures. They go through one Tauri command:

```rust
#[tauri::command]
fn track(event: ClientEvent) // serde-tagged enum, a subset of AnalyticsEvent
```

- The TS side gets `track(event: ClientEvent)` in `src/lib/tauri.ts`, with a TS union that mirrors the Rust enum.
- An unknown variant fails to deserialize and is dropped. The frontend can't invent events or values.

## 4. Events

There are 15 event names. Per user per day, that's in practice 2 to 6 events. At 100 daily users it's under 20k
events a month. PostHog's free tier is 1M a month.

### 4.1 Lifecycle and "is this a real user"

| Event | When | Properties | Origin |
|---|---|---|---|
| `app_installed` | First launch of an install (once). Existing installs updating to 0.7.7 send it with `estimated: true` | `estimated` (bool) | Rust, startup |
| `app_launched` | Every launch | `launch_mode`: `autostart`, `manual`; `previous_exit`: `first_run`, `clean`, `crash`, `after_panic`, `system_restart`, `update`; `previous_phase`: `startup`, `idle`, `dictating`, `updating`; `previous_uptime_min` (number) | Rust, startup (section 5) |
| `first_dictation` | First successful dictation of the install (once) | `latency_ms`, `audio_ms` | Rust, `state/dictation/record.rs` |
| `active_day` | First successful dictation of each UTC day | none | Rust, same place |
| `day_summary` | For each earlier UTC day that had dictations, at the first launch or dictation after it. Its `timestamp` is that day at 23:59:59 UTC | section 4.2 | Rust, section 4.2 |

`active_day` gives a same-day count of people who actually dictated. `day_summary` follows with the details a day
later. If someone never comes back, their last day still counts through `active_day`; only that day's details are
lost.

### 4.2 `day_summary`

This is the "how did their day go" event. All numbers come from the day's dictations on this PC:

| Property | Meaning |
|---|---|
| `dictations` | Dictations started |
| `succeeded`, `empty`, `failed`, `retried` | Outcome counts (empty = no text came back; failed = any `DictationFailureKind` except `no_speech`) |
| `no_speech` | Dictations where nothing was heard |
| `latency_p50_ms`, `latency_p90_ms` | Key release to text inserted, successful dictations only |
| `audio_p50_ms` | Median speaking length |
| `words_total` | Total words inserted (a count; never the words) |
| `gpu_share_pct` | Percent of dictations that ran on the GPU |
| `hands_free` | Dictations started hands-free |
| `insert_typed`, `insert_pasted`, `insert_clipboard_only`, `insert_history_only`, `insert_blocked_elevated` | How the text got in |
| `app_browser`, `app_code_editor`, `app_terminal`, `app_chat`, `app_office`, `app_notes`, `app_ai`, `app_other`, `app_unknown` | Dictations per kind of app (section 4.5) |
| `format_style` | Active style at day end: `standard`, `coding`, `writing`, `casual`, `literal` |

**Accumulator:** new `src-tauri/src/analytics/day.rs`.
- Persisted to `<data_dir>/analytics-day.json` with `atomic_file::atomic_write`. It holds the UTC day, the counters,
  and up to 500 latency and audio samples for the percentiles.
- **Fed from `state/dictation/record.rs`**, next to the existing latency record (L343). Feed it **before** the
  `settings.diagnostics_opt_in` gate there: that gate is for the encrypted diagnostics store, not for this. Failures
  and empties are fed where `DictationFailed` is recorded, and retries where Try again runs.
- On a new UTC day (checked at startup and at each dictation): if the stored day is earlier, queue `day_summary`,
  reset the file, then count the new dictation.
- Raw samples never leave the file.
- When consent is off, the file isn't written at all.

### 4.3 Problems

| Event | When | Properties | Origin |
|---|---|---|---|
| `dictation_failed` | Each failed dictation, at most 20 per day | `kind`: the existing `DictationFailureKind` values; `app_kind` (4.5) when `kind = insert` | Rust, `telemetry::record` |
| `worker_crashed` | A speech helper exits or times out | `worker`: `asr_gpu`, `asr_cpu`, `faster_whisper`, `polish`; `exit`: `crash`, `timeout`; `cause` (4.4) | Rust, `telemetry::record` |
| `app_error` | A Rust panic, reported on the next launch from `panics.log` | `area`: the existing `PanicArea` values | Rust, startup |
| `speech_fallback` | The GPU engine steps down to the CPU | `reason`: `preparing`, `cooldown`, `driver_disabled`, `unavailable`, `fault` | Rust, where `Speech.gpuStepDown` is decided today. Find the source of the TS event and capture there |
| `model_download_finished` | A speech model download ends | `model`: `parakeet`, `whisper_cpp`, `faster_whisper`, `polish`; `outcome`: `installed`, `failed`, `cancelled`; `failure_stage` (the existing `DownloadFailureStage`); `retries`; `downloaded_pct`; `duration_s` | Rust, `ModelManager::finish_download` |
| `update_failed` | The in-app updater throws | `stage`: `download`, `verify`, `install`; `error_kind`: `network`, `signature`, `disk`, `permission`, `other`; `target_version` (semver or `other`) | TS `track`, from `src/lib/updater.ts` (classification as in v1) |
| `hotkey_failed` | The keyboard hook can't be set up | `kind`: `other_instance`, `hook_rejected`, `hook_thread`, `other` | Rust, `state.rs` L2349 to L2370 (mapping as in v1) |

`model_download_finished` with `outcome = installed` is new: today only failures are counted, so the download
success rate can't be known.

### 4.4 Worker crash cause

**What exists** (`src-tauri/src/inference/asr_worker.rs`):
- A drain thread keeps the last 40 stderr lines (`STDERR_TAIL_LINES`, L64).
- `exited()` (about L476) puts only the last 3, cut to 120 characters, into the fault reason.
- The exit code is never read.

**Change:**
1. **Read the exit code.** In `exited()`, after the existing 100 ms wait, call `child.try_wait()`. This needs
   `&mut self` or a lock.
2. **Log the full tail locally.** Write one `warn!` with the exit code and all 40 lines, each passed through
   `support::redact_profile_dir`. It goes to the local log only, and the support file picks it up.
3. **Classify the cause on the PC.** Send it as `cause`:

| Exit code or stderr | `cause` |
|---|---|
| `0xC0000005` | `access_violation` |
| `0xC000001D` | `illegal_instruction` |
| `0xC0000017`, or stderr "failed to allocate" or "out of memory" | `out_of_memory` |
| stderr `ErrorDeviceLost` or `VK_ERROR_DEVICE_LOST` | `gpu_device_lost` |
| stderr `ErrorOutOfDeviceMemory` | `gpu_out_of_memory` |
| `0xC0000135`, `0xC0000139`, `0xC0000142` | `missing_dll` |
| `0xC0000409` | `abort` |
| a nonzero code not listed above | `exit_nonzero` |
| no code | `unknown` |

Do the same in `faster_whisper.rs` (about L798), which keeps 8 lines today.

### 4.5 Kind of app (`src-tauri/src/insertion/mod.rs`)

Add `AppKind` and `app_kind(exe: Option<&str>) -> AppKind` next to `terminal_paste_chord` (L526). Use the mapping
from v1, with `ai_app` renamed `ai`:

- `browser`: chrome, msedge, firefox, brave, opera, vivaldi, arc
- `code_editor`: code, code - insiders, cursor, windsurf, zed, devenv, idea64, pycharm64, webstorm64, rider64,
  sublime_text, notepad++
- `terminal`: `is_terminal_process_exe`
- `chat`: slack, discord, teams, ms-teams, whatsapp, telegram, signal
- `office`: winword, excel, powerpnt, outlook, olk, onenote
- `notes`: notion, obsidian, notepad, evernote, logseq
- `ai`: claude, chatgpt, codex
- `other`: anything not listed
- `unknown`: no foreground process

**Rules:**
- `InsertResult` (L28) gains `#[serde(default)] app_kind: Option<AppKind>`.
- Set it where `insert_text` already resolves the foreground process. If some path doesn't resolve it, call
  `foreground_process_exe_name()` (L610) once at the start of the request, map it, and drop the string.
- The kind travels onward; the exe name never leaves that function.

### 4.6 Choices and feedback

| Event | When | Properties | Origin |
|---|---|---|---|
| `onboarding_step` | Each onboarding step reached | `step`: `welcome`, `mic`, `speak`, `done`; `seconds_on_previous` (number) | TS `track` |
| `onboarding_finished` | Done or skipped | `outcome`: `finished`, `skipped`; `total_s` | TS `track` |
| `consent_answered` | v2 prompt or onboarding | `choice`: `keep_sharing`, `turned_on` (onboarding). A "turn off" sends nothing | TS `track` |
| `acquisition_answered` | Onboarding done step (section 7) | `answer`: the v1 list; also `$set_once: {acquisition: answer}` | TS `track` |
| `feedback_reported` | "Something wrong?" (section 8.1), at most 10 a day | `kind`: `wrong_words`, `missed_words`, `too_slow`, `text_did_not_appear`, `wrong_place`, `froze_or_crashed`, `hotkey`, `other`; `surface`: `pill`, `settings`, `history`; and, for history only, `latency_ms` and `audio_ms` of that dictation | TS `track` |

## 5. Launch facts: unclean exit, update installed or not applied (Rust)

This is v1's section 7, with its outputs folded into `app_launched`.

**New `src-tauri/src/session.rs`.** The session file is `<data_dir>/session.json`, written with `atomic_write`:

```json
{ "v": 1, "appVersion": "0.7.7", "pid": 1234, "startedAtMs": 0, "lastAliveMs": 0,
  "bootAtMs": 0, "phase": "startup", "panicked": false, "updateTarget": null }
```

**At startup** (after the single-instance claim and `support::init()`), read the previous file and work out
`previous_exit`:

| Previous file | `previous_exit` |
|---|---|
| No file, and `is_new_install` | `first_run` |
| No file (a clean exit deleted it) | `clean` |
| `phase = updating` and `appVersion` < current | `update` (also: `$set` app_version) |
| `phase = updating` and `appVersion` == current | Sends `update_failed { stage: not_applied }`; `previous_exit = clean` |
| `panicked = true` | `after_panic` |
| `abs(prev.bootAtMs - current bootAtMs) > 120 s` | `system_restart` |
| otherwise | `crash` |

- `bootAtMs = now - GetTickCount64()`, rounded to the minute.
- `previous_phase` comes from the old file, and `previous_uptime_min = (lastAliveMs - startedAtMs) / 60000`.
- A corrupt file is treated as absent.
- If the previous `pid` is alive and belongs to VoiceWave, ignore the file (defensive; single instance should
  prevent this).

**While running:**
- A heartbeat thread rewrites `lastAliveMs` every 5 min.
- `phase` becomes `dictating` when capture starts (`state/dictation/capture.rs`) and `idle` when the session ends. To
  limit disk writes, write a phase change at most every 10 s.
- `support.rs`'s panic hook (L96) sets `panicked: true` as a best effort, and must never panic itself.
- `RunEvent::Exit` (lib.rs L2654) deletes the file next to `shutdown_inference_workers()`.

**Updating:**
- New command `mark_update_installing(target_version)`. It validates the semver and sets `phase: updating`.
- `src/lib/updater.ts` `installPendingUpdate` (L57) calls it before `downloadAndInstall`, and calls
  `clear_update_installing()` if that throws.

**Unclean exit banner:** on the next launch, for `crash` or `after_panic` only. It's shown once, and doesn't depend
on consent. Rust exposes `previous_exit_unclean` in `SupportContext`. Copy is in section 8.3.

## 6. Install source (unchanged from v1)

v1's section 3 stands as written. In short:
- The NSIS `NSIS_HOOK_POSTINSTALL` writes `InstallSource` (`winget` or `scoop` from `/VWSOURCE=`, otherwise `direct`
  for an interactive install or `unknown` for a silent one) to `HKCU\Software\VoiceWave\LocalCore`. The first writer
  wins.
- `support::install_source()` reads it.
- The winget manifest gains `Custom: /VWSOURCE=winget` once, by hand, on the 0.7.7 winget PR. komac carries it
  forward from there.
- Both release workflows get a guard step that warns when the latest merged manifest has lost the switch.
- `NSIS_HOOK_POSTUNINSTALL` deletes the whole key only when "Delete app data" is ticked and it isn't an update. That
  key now also holds `InstallId` and the first-seen facts.

All of v1's verified facts still apply: winget passes Custom switches in every mode, `--override` drops them, komac
carries switches forward since v2.12.0, and NSIS ignores switches it doesn't know. So do v1's test rows and the risk
from the damn-good-b0t bot dropping the switch (v1 3.5). Wherever this plan says "v1 section N", read
`plans/artifacts/telemetry-2026-10/024-v1-telemetrydeck-spec.md`.

## 7. Acquisition question (unchanged from v1)

- It goes on the onboarding done step, and only when `is_new_install` and the switch is on.
- Single-select chips (`role="radiogroup"`), no free text, and it's never shown again.
- Leaving the step without choosing sends `skipped` once.

Choices and values, in order:

| Choice | Value |
|---|---|
| Reddit | `reddit` |
| X | `x` |
| YouTube | `youtube` |
| GitHub | `github` |
| Google or another search engine | `search` |
| ChatGPT or another AI assistant | `ai_assistant` |
| Hacker News | `hacker_news` (new) |
| AlternativeTo | `alternativeto` |
| SaaSHub | `saashub` |
| DevHunt | `devhunt` |
| A friend or coworker | `friend` |
| Browsing winget | `winget` |
| A blog or article | `article` |
| Somewhere else | `other` |
| Skip | `skipped` |

Hacker News was added for the 2026-10-27 Show HN.

## 8. Diagnostics and problem reports (UI)

### 8.1 "Something wrong?"

- **Surfaces:** the pill's quick menu, Settings → Help (next to Report a problem), and "Report this dictation" on a
  History row. **[HIS CALL]** which of the three.
- **Flow:**
  1. The user picks one reason (the `kind` list in 4.6).
  2. If the switch is on, `feedback_reported` is sent.
  3. A thanks screen offers **Tell us more on GitHub** and **Save support file**:
     - "Tell us more on GitHub" opens `issue_report_url` (support.rs L420), pre-filled with the reason, the app
       version and the speech path.
     - "Save support file" is 8.2.
  4. With the switch off, steps 1 and 3 still work, and the thanks screen says nothing was sent.
- **No free text in analytics.** Free text only goes into a GitHub issue the user writes themselves.

### 8.2 Support file (unchanged from v1 section 9)

- A new command, `export_support_zip`, writes `%USERPROFILE%\Downloads\voicewave-support-YYYYMMDD-HHMM.zip`.
- It needs the `zip` crate (`default-features = false, features = ["deflate"]`).

| Entry | Contents |
|---|---|
| `report.txt` | The text that Copy diagnostics produces |
| `diagnostics.json` | The diagnostics bundle, **not** gated on `diagnostics_opt_in` here |
| Logs | The newest 3 daily logs, each cut to its last 2 MB, every line passed through `redact_profile_dir` |
| `panics.log` | A copy, without draining it |
| `session.json` | Only when the last exit was unclean |
| `README.txt` | Fixed text |

- **Size:** at most 8 MB.
- **Never included:** settings, history, dictionary, snippets, models or the secure store.
- **"Open logs folder"** (already at App.tsx L1355) is also reused next to this button and in the unclean exit
  banner.

### 8.3 Copy (no dashes anywhere)

**Existing user v2 dialog.** This replaces the dialog at App.tsx L3737 to L3765. Use `aria-label="Privacy check"`.
Focus starts on "Keep sharing". Escape and a backdrop click do nothing.

```text
Title:    A quick privacy check
Body:     VoiceWave 0.7.7 sends a little more about how the app is working for you, so we can find and fix
          problems. It is anonymous: a random ID, never your name or account.
What:     Each day you use it: how many dictations worked or failed, how fast text appeared, and the kind of
          app it went into (such as a browser or a code editor, never its name). Also setup progress, model
          downloads, updates, crashes and your answers to our two optional questions.
Never:    Never sent: audio, transcripts, dictionary or snippet content, window titles, app names, file paths,
          device names, error messages, or your IP address.
Buttons:  [Keep sharing]   [Turn off]
Footer:   You can change this anytime in Settings → Diagnostics.
```

- "Keep sharing" sets version 2, sends `consent_answered { keep_sharing }`, and lets the outbox send.
- "Turn off" sets the switch off and version 2, purges the outbox, and sends nothing.

**Onboarding welcome toggle** (Onboarding.tsx, the existing toggle):

```text
Title:  Anonymous usage and problem reports · on by default
Body:   Tells us how setup and dictation go, with a random ID. Never audio, transcripts or app names.
Link:   What's sent
```

**Settings → Diagnostics** (App.tsx about L3111):

```text
Toggle:  Anonymous usage and problem reports
         How setup, dictation, downloads and updates go for you, with a random ID. Never audio,
         transcripts or app names.
Support file heading:  Support file
Support file body:     Saves a zip file with your diagnostics report and recent logs to your Downloads
                       folder. Nothing is uploaded. Attach it to a GitHub issue if you want help.
Button:                Save support file      After save:  Saved to {path}  [Show in folder]
Error:                 Couldn't save the support file. You can still open the logs folder.
```

**Unclean exit banner:**

```text
VoiceWave closed unexpectedly last time. If it keeps happening, save a support file and attach it to a
GitHub issue.   [Save support file]  [Open logs folder]  [Dismiss]
```

**Update failure line** (UpdatePrompt and UpdateSection):

```text
The update didn't install. VoiceWave will try again later, or you can download the latest version from
voicewave.site.
```

**`ANONYMOUS_USAGE_DETAILS`** (telemetry.ts L22, shown under "What's sent"):

```text
A random install ID and a random ID for each launch; the app version; Windows 10 or 11; your graphics card
maker; whether speech runs on the graphics card or the processor and which model family; how you installed
VoiceWave (winget, Scoop, a direct download, or unknown) and how many days ago; whether the PC is a virtual
machine. Events: installed, launched (and whether the last run ended normally, crashed, or was cut off by a
restart or an update), setup steps, your first dictation, and once for each day you dictate: how many
dictations worked, came back empty or failed, how fast text appeared, how long you spoke, how many words were
inserted, how the text got in, and how many went into each kind of app (browser, code editor, terminal, chat,
office, notes, AI app, or other). Also failed dictations by type, speech helper crashes by cause, model
downloads and how they ended, graphics card to processor fallbacks, failed updates, shortcut setup problems,
internal errors by area, and your answers to "Where did you hear about VoiceWave?" and "Something wrong?" if
you give them. Never audio, transcripts, dictionary or snippet content, window titles, app names, file paths,
device names, error messages, account details, or your IP address.
```

## 9. PostHog dashboard (build after the first events)

**Definitions** (exclude `hypervisor` in `vmware`, `virtualbox`, `kvm`, `xen`, `parallels`, `other` everywhere):

| Term | Rule |
|---|---|
| Install | A person with `app_installed` |
| Activated | A person with `first_dictation` |
| **Real user** | A person with `active_day` on 2 or more different days. Save it as a cohort |
| Active user (day, week) | A person with `active_day` in the period |
| Retained (week N) | Retention insight: `app_installed`, then `active_day` |

**Insights:**

| Question | Insight |
|---|---|
| How many users do we have? | Weekly unique people with `active_day`, plus the Real user cohort size |
| How many installs become users? | Funnel: `app_installed`, `onboarding_finished`, `first_dictation`, `active_day` on a second day |
| Where does setup lose people? | Funnel over `onboarding_step` |
| Is dictation working? | `sum(failed) / sum(dictations)` from `day_summary`, by `speech_path` and `model_family` |
| Is it fast? | Median `latency_p50_ms` from `day_summary`, by `speech_path` |
| Does it crash? | `app_launched` where `previous_exit` is `crash` or `after_panic`, over all launches, by `app_version` |
| Do downloads work? | `model_download_finished` by `outcome` and `failure_stage` |
| Do updates land? | `app_launched` with `previous_exit = update`, against `update_failed`, by version |
| Where do people come from? | `acquisition_answered` by `answer`; `install_source` |
| What hurts? | `feedback_reported` by `kind` |

One person's timeline is PostHog's Persons view, searched by an event property.

## 10. UTM links (unchanged from v1 section 10, no release needed)

Start now. The convention and the table of links are in v1. Always link the apex `https://voicewave.site/`, never
`www`. Add a row for HN:

`https://voicewave.site/?utm_source=hackernews&utm_medium=social&utm_campaign=show-hn-2026-10-27`

## 11. README, privacy notice, website, changelog

**`README.md`** L104 to L126: replace the "Anonymous usage count" section.
- It names PostHog (EU), says IPs are discarded, and lists the same facts as `ANONYMOUS_USAGE_DETAILS` as `*` bullets.
- Add the support file paragraph from v1 11.1.

**New `docs/PRIVACY.md`:** v1 11.2, with "TelemetryDeck" replaced by "PostHog (EU Cloud), with IP addresses
discarded". For 0.7.7 only, also say: "0.7.7 also still sends the older anonymous usage count through TelemetryDeck;
that stops in the next release."

**Website FAQ** (`voicewave-website/src/components/Faq.tsx` L12 to L13), as one paragraph:

```text
VoiceWave sends anonymous usage and problem reports (a random install ID, how setup and dictation go each
day, crashes and failed updates) through PostHog with IP addresses discarded. One switch turns it off.
Never audio, transcripts, app names or file paths.
```

**CHANGELOG `## [0.7.7]`:**

```text
### Privacy and diagnostics
* Usage and problem reports move to PostHog (EU), with IP addresses discarded, and now tell us whether
  dictation works and how fast it is each day, so we can find problems you hit. Asked once; one switch
  turns everything off.
* New: Settings → Diagnostics → Save support file, and a "Something wrong?" button.
* New: an optional "Where did you hear about VoiceWave?" question during setup.
* Reports are kept and sent later when you're offline, instead of being lost.
```

## 12. Files

| File | Change |
|---|---|
| `src-tauri/src/analytics/` (new) | Section 3, plus `day.rs` (4.2) |
| `src-tauri/src/net.rs` (new, moved) | Shared ureq agent and TLS config from `model_manager/net.rs` |
| `src-tauri/src/session.rs` (new) | Section 5 |
| `src-tauri/src/telemetry.rs` | `record()` also calls `analytics::capture`; `WorkerCrashed` gains `cause` |
| `src-tauri/src/support.rs` | `install_source()`; panic hook marks the session |
| `src-tauri/src/insertion/mod.rs` | `AppKind`, `app_kind()`, `InsertResult.app_kind` |
| `src-tauri/src/inference/asr_worker.rs`, `faster_whisper.rs` | Exit code, full tail logged, `cause` |
| `src-tauri/src/state/dictation/record.rs`, `capture.rs`, `insert.rs` | Day accumulator, `first_dictation`, `active_day`, session phase |
| `src-tauri/src/state.rs` L2349 to L2370 | `hotkey_failed` |
| `src-tauri/src/state/gpu_engine.rs` | `set_context` on status changes; `speech_fallback` |
| `src-tauri/src/model_manager/mod.rs` | `model_download_finished` in `finish_download` |
| `src-tauri/src/settings/mod.rs` | Consent version 2, `analytics_consent_prompt_shows`, new migration, `set_consent` calls |
| `src-tauri/src/diagnostics/mod.rs` | A bundle builder without the opt-in gate, for the zip |
| `src-tauri/src/lib.rs` | `mod analytics; mod session; mod net;`, startup wiring, the `track`, `mark_update_installing`, `clear_update_installing`, `export_support_zip` and `reveal_in_folder` commands, `RunEvent::Exit` flush and clear, `SupportContext` fields |
| `src-tauri/Cargo.toml` | `zip` |
| `src-tauri/windows/hooks.nsh` | Install source (section 6) |
| `.github/workflows/release.yml` | `VOICEWAVE_POSTHOG_KEY` and `VOICEWAVE_POSTHOG_HOST` build env from secrets; manifest guard step |
| `.github/workflows/winget-release.yml` | Manifest guard step |
| `src/lib/tauri.ts`, `src/types/voicewave.ts` | `track`, `ClientEvent`, new commands, settings and `SupportContext` fields |
| `src/lib/telemetry.ts` | New `ANONYMOUS_USAGE_DETAILS`; consent version 2; TelemetryDeck sending otherwise unchanged |
| `src/lib/updater.ts`, `UpdatePrompt.tsx`, `UpdateSection.tsx` | Update marking, `update_failed`, new error copy |
| `src/hooks/useVoiceWave.ts` | `track` calls for onboarding steps and timing |
| `src/App.tsx` | v2 dialog, Settings toggle copy, support file block, unclean exit banner, Something wrong |
| `src/components/Onboarding.tsx` | Toggle copy, acquisition question |
| `src/components/FloatingPill.tsx` (quick menu), the History row | Something wrong entry points |
| `README.md`, `docs/PRIVACY.md` (new), `CHANGELOG.md`, `voicewave-website/src/components/Faq.tsx` | Section 11 |

## 13. Phases and how it gets built

| Phase | What | Depends on | Effort |
|---|---|---|---|
| P0 | UTM links (section 10) | nothing; no release | 30 min |
| P1 | Analytics core: module, identity, context, hypervisor check, outbox, sender, consent gate, `track`, shared `net`, release.yml env | nothing | 2 days |
| P2 | Consent v2: settings migration, dialog, onboarding and Settings copy | P1 | 1 day |
| P3 | Events: `session.rs` and `app_launched`, day accumulator, `first_dictation` and `active_day`, problem events, worker cause, `AppKind`, download, hotkey, fallback, onboarding `track` calls, `update_failed` | P1 | 2 to 2.5 days |
| P4 | Install source (NSIS, guard steps) and acquisition question | P1 for `install_source` in base properties | 1 day |
| P5 | Something wrong, support file, unclean exit banner | P3 for the banner | 1.5 days |
| P6 | README, PRIVACY, FAQ, changelog, PostHog dashboard, the manual checklist, release | all | 1 day |

That's about 8 to 9 days for one person.
- **With subagents:** P1 first, alone. Then P3, P4 and P5 in parallel worktrees, merged one at a time into
  `release/0.7.7` (they share `lib.rs` and `App.tsx`). Then P2 and P6. Each phase is reviewed before it's merged.
- **Rithwik's external steps:** section 3.6 (PostHog project, IP setting, repo secret), and the one-time winget
  manifest edit (section 6).

## 14. Manual test checklist

No new unit tests, per the owner's rule. These run on a dev build with `VOICEWAVE_POSTHOG_DEV_KEY` pointing at the
`VoiceWave dev` project, plus Windows Sandbox for installer checks.

**Core:**
- [ ] A new install sends `app_installed` and `app_launched { previous_exit: first_run }` once, with an `InstallId`
  in the registry.
- [ ] Offline: dictate 5 times, quit, relaunch online. The events arrive with their original timestamps, and
  `analytics-outbox.json` empties.
- [ ] A 413 from a mock host drops the batch. A 503 keeps it and backs off (watch the log).
- [ ] Turning the switch off deletes the outbox at once, and nothing more is sent.
- [ ] A debug build without the dev key sends nothing.
- [ ] The PostHog event view shows no IP or GeoIP properties.

**Consent:**
- [ ] An existing opted-in profile (version 1) sees the dialog once. "Keep sharing" sends the queued events.
  "Turn off" purges them, and TelemetryDeck stops too.
- [ ] An existing opted-out profile: no dialog, nothing sent.
- [ ] Quitting with the dialog open 3 times: no fourth prompt, PostHog stays off, and TelemetryDeck continues.

**Events:**
- [ ] Dictate 10 times (2 silent, 1 into Chrome, 1 into Windows Terminal). Move the clock to the next day and
  launch. One `day_summary`, dated the previous day, shows the right counts and app kinds, and `analytics-day.json`
  resets.
- [ ] `active_day` is sent once per day, and `first_dictation` once ever.
- [ ] Kill VoiceWave in Task Manager, then relaunch: `previous_exit = crash`. A clean tray Quit, then relaunch:
  `clean`. Reboot with it running: `system_restart`.
- [ ] Update to a test build: `previous_exit = update`. With a read-only install dir: `update_failed { stage:
  not_applied }`.
- [ ] Kill `voicewave-asr-vk.exe` mid-dictation: `worker_crashed` with a `cause`, and the log has the exit code and
  the 40 lines.
- [ ] A successful Parakeet download sends `outcome: installed` with `duration_s`. A cancelled one sends
  `cancelled`.
- [ ] Two instances in dev with single instance bypassed: `hotkey_failed { other_instance }`.
- [ ] A Windows Sandbox install reports `hypervisor: microsoft`. A VMware or VirtualBox VM, if one's available,
  reports its own.

**Install source, acquisition, feedback, support file:** v1's section 13 rows, plus:
- [ ] Something wrong from each surface sends its `surface`. The 11th report in a day isn't sent.
- [ ] The support file opens, holds only the listed entries, and doesn't contain your Windows user name.

**Regression:**
- [ ] TelemetryDeck still receives today's events, with unchanged names and values.

**Copy:**
- [ ] No `-`, `–` or `—` in any new user-facing string. Check by eye and with a quick `rg` over the changed TSX and
  TS strings.

## 15. Open questions

1. ~~Hosting~~: decided 2026-10-28, EU Cloud (section 3.6).
2. **[HIS CALL] Something wrong surfaces:** pill, Settings, History row; which ones?
3. **TelemetryDeck removal:** proposed for the first 0.8.x release after PostHog has a week of data. Agree?
4. **Acquisition list:** anything to add or drop? Values are fixed at release.
5. **App kind list (4.5):** the right set?
6. **damn-good-b0t** (v1 3.5): close its PRs if they drop the switch, or ask its owner?

## 16. Sources

- PostHog capture API: `/batch/`, EU host `https://eu.i.posthog.com`, US `https://us.i.posthog.com`, request body
  under 20 MB: https://posthog.com/docs/api/capture
- PostHog event dedupe on uuid, event, timestamp and distinct id (not guaranteed to be instant):
  https://posthog.com/docs/cdp/troubleshooting.md and https://posthog.com/docs/data/ingestion-warnings
- PostHog IP capture setting (Settings → Project → General → IP data capture; EU organizations default to discard):
  https://posthog.com/docs/privacy/data-collection
- PostHog pricing, 1M product analytics events free each month: https://posthog.com/pricing
- T3 Code's telemetry design (allowlisted values at the source, uuid on every event with bounded jittered retries,
  local failure buffers), read at commit `9bc4138` on 2026-10-28: https://github.com/pingdotgg/t3code
- v1 sources (winget Custom switches, komac, the Tauri NSIS template, GoatCounter campaigns) are unchanged and listed
  in v1's section 15.
