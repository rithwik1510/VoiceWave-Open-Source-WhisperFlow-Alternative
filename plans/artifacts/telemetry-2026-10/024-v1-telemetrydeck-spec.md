# VoiceWave 0.7.7: telemetry and diagnostics build spec

Written 2026-10-28 (IST). Rithwik implements this himself. The baseline is the VoiceWave repo at the 0.7.6 merge,
`9a214a1ef4495c4a928ae50733c8d8fbf52dd552`. Every path and line number below was checked against that commit.
Background and sources are in `t3code-telemetry-research-2026-10-28.md`, the same folder as this file.

Decisions from Rithwik that this spec follows:
1. **Where did you hear about VoiceWave:** an optional first-launch question. Multiple choice only, no free text, new installs only.
2. **Install source:** an explicit marker only. The winget manifest adds a custom switch `/VWSOURCE=winget`, and the NSIS
   installer saves it. A silent install is never counted as winget. Anything without the marker reports `direct` or
   `unknown`. Auto updates keep the original source.
3. **Consent prompt v2:** shown once to existing users after the update, and to new users during onboarding. It gives the
   exact list of what is collected and the choices, and it says what decline means. All copy has no dashes.
4. **Privacy:** values come from fixed lists only. Never audio, transcripts, paths, window titles or app names.

Items marked **[HIS CALL]** are proposals that need Rithwik's decision before he builds them.

---

## 0. Summary of phases

| Phase | What | Release needed | Effort (solo) |
|---|---|---|---|
| P0 | UTM-tagged links (section 10) | No, start today | 30 min |
| P1 | Install source marker: NSIS hook, Rust reader, winget manifest switch, workflow check | Yes | 0.5 to 1 day |
| P2 | Persistent outbox with retry, new base properties, consent v2 (settings, migration, prompt, onboarding) | Yes | 2 to 2.5 days |
| P3 | New events: App.firstLaunch, Acquisition.sourceReported, App.uncleanExit, Update.failed, Hotkey.registerFailed, Insertion.targetDaily | Yes | 2 days |
| P4 | Support zip ("Save support file") and wiring Open logs folder into more places | Yes | 1 day |
| P5 | README, privacy notice, website FAQ, changelog, tests, release, first winget PR with the switch | Yes | 1 day |

Total is about 7 to 8 working days. P1 and P4 do not depend on P2, so they can land first. P3 depends on P2's outbox
and consent tiers.

---

## 1. What already exists at 9a214a1 (so it is not rebuilt)

- **Rust event queue:** `src-tauri/src/telemetry.rs`.
  - In-memory `QUEUE` with `QUEUE_CAP = 64` (L16).
  - `record()` emits `voicewave://telemetry-pending` (L163).
  - `drain_telemetry` (L182) clears the queue and `panics.log`.
  - Event enum `TelemetryEvent` (L26): `DictationFailed`, `WorkerCrashed`, `Panicked`.
- **Frontend sender:** `src/lib/telemetry.ts`.
  - TelemetryDeck SDK client with `testMode: import.meta.env.DEV` (L143).
  - `prepareSignal` (L177) hardcodes `"VoiceWave.Distribution.channel": "github"`.
  - `sendFunnelEvent` (L298) writes the dedupe marker only after `response.ok`. A failed send is simply lost, and
    events drained from Rust are lost too (useVoiceWave.ts L2524 comment: "Draining always clears the queue").
- **Consent:**
  - `anonymous_usage_opt_in` and `analytics_consent_version` live in `src-tauri/src/settings/mod.rs`
    (default L274, `ANALYTICS_CONSENT_VERSION = 1` at L403, `migrate_analytics_consent` at L409).
  - The TS mirror is `ANALYTICS_CONSENT_VERSION` in telemetry.ts L19.
  - Prompt gating is `showAnalyticsConsent` in App.tsx about L829, and the dialog is at App.tsx L3737 to L3765.
  - The onboarding toggle is in Onboarding.tsx (welcome step, "Anonymous usage count · on by default").
- **Diagnostics:**
  - `export_diagnostics_bundle` (lib.rs L1592, diagnostics/mod.rs L340) writes a JSON file and only works when
    `diagnostics_opt_in` is on.
  - `get_diagnostics_report` (lib.rs L1247) and `open_issue_report` (lib.rs L1256) exist.
- **Open logs folder already exists.** The `open_logs_folder` command is at lib.rs L1267, the `openLogsFolder` TS
  wrapper at tauri.ts L408, and `showLogsFolder` at App.tsx L1355. The button is in Settings → Diagnostics → Report a
  problem (App.tsx about L3205). P4 only reuses it in new places.
- **Logs:** `support::log_dir()` (support.rs L34) points to `%LOCALAPPDATA%\voicewave\localcore\data\logs`. Logs roll
  daily, 7 are kept, and `panics.log` stays capped at 4 KB.
- **Clean exit hook:** `RunEvent::Exit` in lib.rs L2651 to L2656 already calls `shutdown_inference_workers()`.
- **Single instance:** `tauri-plugin-single-instance` is claimed before setup (lib.rs about L2385). A second launch never
  reaches setup, which matters for the session lock.
- **Hotkey runtime failure:** state.rs L2349 to L2370 already classifies "already owns the keyboard hook" and sets
  `set_registration_error`.
- **Insertion:** `foreground_process_exe_name()` (insertion/mod.rs L610) and `terminal_paste_chord()` (L526), plus
  `InsertResult.blocked_by_elevation` (L39) and `InsertionMethod { Direct, ClipboardPaste, ClipboardOnly,
  HistoryFallback }` (L10).
- **Updater:** `src/lib/updater.ts` `installPendingUpdate` (L57) calls `downloadAndInstall`. Callers are
  `UpdatePrompt.tsx` L94 and `UpdateSection.tsx` L72. tauri.conf.json sets `installMode: passive`.

---

## 2. Privacy rules (apply to every phase)

1. **Every property value comes from a closed list in code.**
   - In Rust, that means a `#[serde(rename_all = "kebab-case")]` enum.
   - In TS, it means a string union plus a runtime guard, `pick(value, ALLOWED, "other")`, applied in
     `describeFunnelEvent`.
   - The only non-enum values are the app version and `Update.targetVersion`. Both must match
     `^\d+\.\d+\.\d+$`, or `"other"` is sent instead.
2. **Never sent:** audio, transcript text, window titles, exe names, app names, file paths, device names, error
   message text, dictionary or snippet content, or settings values.
   - The exe name is mapped to a category inside Rust, in the same function that reads it, and the raw name is
     dropped.
   - Error text is mapped to a kind locally and never stored in the outbox.
3. **Dev builds do not count.** Keep `testMode: import.meta.env.DEV`, and skip session lock events in
   `cfg(debug_assertions)`.
4. **Gate at enqueue time and again at send time.**
   - Turning a tier off deletes that tier's queued events right away.
5. **The support zip stays local.** It is user-initiated and written to disk only. VoiceWave never uploads it.

---

## 3. Install source (P1)

### 3.1 Verified facts

- **winget passes Custom switches in every install mode.**
  - Schema 1.12.0 lists `InstallerSwitches.Custom`: "switches the Windows Package Manager will pass to the installer in
    addition to Silent, SilentWithProgress, and Interactive"
    (winget-pkgs `doc/manifest/schema/1.12.0/installer.md`). winget-cli issue #239 says the same: Custom is passed
    regardless of mode.
  - Caveat: `winget install --override` drops all manifest switches. Those installs fall back to `unknown`, which is the
    honest result.
- **NSIS can read the switch in a Tauri hook.**
  - The Tauri 2 template (`crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi`) includes `FileFunc.nsh` (L23).
  - It already parses `$CMDLINE` with `${GetOptions}` for `/P` and `/UPDATE` (L511, L521), and it inserts
    `NSIS_HOOK_POSTINSTALL` after the files are copied (about L778).
  - `$UpdateMode` and `$PassiveMode` are global Vars, so a hook can read them. NSIS ignores switches it does not know,
    so old installers that receive `/VWSOURCE=winget` are unaffected.
- **The in-app updater adds `/UPDATE`.** hooks.nsh documents this, and the template skips the reinstall page when
  `/UPDATE` is set.
  - A silent `winget upgrade` (`/S`) skips pages, so the old uninstaller does not run.
  - An interactive install over an old version can run the old uninstaller without `/UPDATE` (template L366 to L376).
    That is why the marker must not be deleted on every uninstall. See 3.3.
- **komac carries switches forward.** komac (the engine inside `vedantmgoyal9/winget-releaser@v2`) copies
  `InstallerSwitches` from the previous manifest on update. This was fixed in komac v2.12.0: "Fix installer switches not
  being carried over from the previous manifest", and later "Fix previous custom switches getting overwritten". The
  0.7.6 manifest was made by komac v2.17.1.
  - So once one merged VoiceWave.LocalCore manifest contains the switch, our workflow keeps it automatically.
  - Neither workflow can set it today: winget-releaser@v2 has no switches input.
- **Who submits.**
  - `damn-good-b0t` opened the 0.6.0, 0.6.1 and 0.7.0 PRs. Its 0.7.0 manifest header reads "Created with winmatsch",
    not komac.
  - Since 0.7.5 the PRs come from `rithwik1510` through our own workflow (winget-pkgs #461240 and #462677).
  - Whether damn-good-b0t keeps Custom switches is **not verified**. See 3.5.

### 3.2 Values

`VoiceWave.Install.source` takes one of:

| Value | Meaning |
|---|---|
| `winget` | The first 0.7.7+ installer that ran saw `/VWSOURCE=winget` |
| `scoop` | Same, with `/VWSOURCE=scoop`, for when the Scoop manifest ships |
| `direct` | An interactive install, without `/UPDATE` and without the switch (a download from the site or GitHub) |
| `unknown` | A silent install without the switch, an `--override` install, a pre-0.7.7 install that was updated in place, or a missing key |

### 3.3 Storage

Use a dedicated per-user registry key, `HKCU\Software\VoiceWave\LocalCore`:

| Value | Type | Written by | Notes |
|---|---|---|---|
| `InstallSource` | REG_SZ | NSIS POSTINSTALL | Written only when the value is absent. Never overwritten |
| `InstallSourceBy` | REG_SZ | NSIS | Installer version (`${VERSION}`), for debugging |
| `FirstSeenUtcMs` | REG_QWORD | Rust, first launch | See section 6 |
| `FirstSeenEstimated` | REG_DWORD | Rust | 1 when backfilled for a pre-0.7.7 install |
| `FirstSeenVersion` | REG_SZ | Rust | App version at first launch |

Why the registry and not a file in `$INSTDIR`:
- It survives in-app updates and reinstalls.
- NSIS writes it with one line, and Rust already links `Win32_System_Registry` (Cargo.toml L45).

Delete the key only when the user ticks "Delete app data" in a real uninstall, which is the same rule the template uses
for `$APPDATA\${BUNDLEID}`.

### 3.4 Files and changes

**`src-tauri/windows/hooks.nsh`:** add the macro, then extend `NSIS_HOOK_POSTUNINSTALL`:

```nsis
!macro NSIS_HOOK_POSTINSTALL
  ; Install source marker (plan 022b). First writer wins, so auto updates and
  ; later winget upgrades keep the original source.
  ClearErrors
  ReadRegStr $R1 HKCU "Software\VoiceWave\LocalCore" "InstallSource"
  ${If} ${Errors}
  ${OrIf} $R1 == ""
    StrCpy $R0 ""
    ClearErrors
    ${GetOptions} $CMDLINE "/VWSOURCE=" $R0
    ${If} $R0 == "winget"
    ${OrIf} $R0 == "scoop"
      ; allowlisted explicit marker
    ${ElseIf} $UpdateMode = 1
      StrCpy $R0 ""            ; in-app update of a pre-0.7.7 install: leave unknown
    ${ElseIf} ${Silent}
      StrCpy $R0 "unknown"     ; silent but unmarked: never guess winget
    ${Else}
      StrCpy $R0 "direct"      ; interactive installer run by a person
    ${EndIf}
    ${If} $R0 != ""
      WriteRegStr HKCU "Software\VoiceWave\LocalCore" "InstallSource" "$R0"
      WriteRegStr HKCU "Software\VoiceWave\LocalCore" "InstallSourceBy" "${VERSION}"
    ${EndIf}
  ${EndIf}
!macroend

; inside the existing NSIS_HOOK_POSTUNINSTALL, append:
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    DeleteRegKey HKCU "Software\VoiceWave\LocalCore"
  ${EndIf}
```

Notes for this hook:
- `${GetOptions}` returns the text after `/VWSOURCE=` up to the next ` /`, and the allowlist rejects anything else.
- Confirm that `${VERSION}` is the define the template uses. If it is not, use `${PRODUCTVERSION}` or drop
  `InstallSourceBy`.
- Confirm in the generated `installer.nsi` that `$DeleteAppDataCheckboxState` is declared before the hook expands.
  Tauri copies the generated script to `target/release/nsis/x64/installer.nsi`.

**`src-tauri/src/support.rs`:**
- Add `pub fn install_source() -> &'static str`. It reads the value with `RegGetValueW(HKEY_CURRENT_USER, ...)`,
  matches it against `winget | scoop | direct | unknown`, and returns `"unknown"` for anything else or a missing value.
- Off Windows, it returns `"unknown"`.

**`src-tauri/src/lib.rs`:**
- Extend `SupportContext` (L1175) with `install_source: String` and `install_age_bucket: String`, both serialized
  camelCase.
- Fill them in `get_support_context` (L1190).

**`src/types/voicewave.ts`:** add `installSource` and `installAgeBucket` to the `SupportContext` type, wherever it is
declared.

**`src/hooks/useVoiceWave.ts` L2494 to L2511 (`setTelemetryProviders` context):** add
`"VoiceWave.Install.source"` and `"VoiceWave.Install.ageBucket"` from `supportContextRef.current`, or `"unknown"` when
it is null.

**`src/lib/telemetry.ts` L184:** keep `"VoiceWave.Distribution.channel": "github"` unchanged so existing charts stay
continuous. It describes the update channel. The new property describes the install method.

**Winget manifest, first time only.** After the 0.7.7 release workflow opens its winget-pkgs PR, push one commit to
that PR branch (in the `rithwik1510/winget-pkgs` fork) so the installer node reads:

```yaml
    InstallerSwitches:
      Silent: /S
      SilentWithProgress: /S
      Custom: /VWSOURCE=winget
```

- Put this in the PR description: "Adds a custom switch so the installer records that it came from winget, for the
  app's anonymous install count. No change to install behaviour."
- Pushing to the PR and writing in it is an external action, so Rithwik does it himself.
- From 0.7.8 on, komac carries the switch forward on its own.

**`.github/workflows/release.yml` (publish-winget job, before the winget-releaser step at L336) and
`.github/workflows/winget-release.yml` (before L63):** add a guard step that warns when the switch has been lost:

```yaml
      - name: Check the last merged manifest still carries /VWSOURCE=winget
        shell: pwsh
        run: |
          $base = "https://raw.githubusercontent.com/microsoft/winget-pkgs/master/manifests/v/VoiceWave/LocalCore"
          $versions = (Invoke-RestMethod "https://api.github.com/repos/microsoft/winget-pkgs/contents/manifests/v/VoiceWave/LocalCore") |
            ForEach-Object { $_.name } | Sort-Object { [version]$_ }
          $latest = $versions[-1]
          $yaml = Invoke-RestMethod "$base/$latest/VoiceWave.LocalCore.installer.yaml"
          if ($yaml -notmatch '/VWSOURCE=winget') {
            "::warning::winget manifest $latest has no /VWSOURCE=winget Custom switch. Add it to the new PR by hand."
          }
```

The first run, for 0.7.7, warns as expected because 0.7.6 has no switch. That warning is the reminder to make the manual
edit.

**Scoop draft** (`/workspace/voicewave/scoop/voicewave.json`, not in the repo): when it ships, change the installer
`ArgumentList` to `'/S /VWSOURCE=scoop'`.

### 3.5 damn-good-b0t risk

If damn-good-b0t submits a version before our workflow does, and its generator (winmatsch) drops Custom, the next komac
run starts from a manifest without the switch. The guard step catches that.

Mitigation for Rithwik, all manual external actions:
- When a bot PR for VoiceWave.LocalCore appears, check its `installer.yaml` for the switch.
- If the switch is missing, comment on that PR or close it in favour of ours.

---

## 4. Event model (P2 and P3)

### 4.1 Consent tiers

| Tier | Setting | Contents |
|---|---|---|
| `basic` | `anonymous_usage_opt_in` (existing) | Everything 0.7.6 sends today, unchanged, plus `App.firstLaunch`, `Acquisition.sourceReported`, and the new base properties `Install.source` and `Install.ageBucket` |
| `reliability` | `reliability_reports_opt_in` (new) | `App.uncleanExit`, `Update.failed`, `Hotkey.registerFailed`, `Insertion.targetDaily`, and the `VoiceWave.Insert.targetCategory` property on `Dictation.failed` with `failureKind=insert` |

- Reliability needs basic. When basic is off, reliability is forced off and its toggle is disabled.
- **Why the existing failure events stay in basic:** `Dictation.failed`, `Model.downloadFailed`, `Speech.*` and
  `App.panicked` were disclosed and accepted under consent v1. Moving them would cut data from existing users with no
  privacy gain.

### 4.2 Base properties on every event

| Property | Values |
|---|---|
| `VoiceWave.App.version` | semver (existing) |
| `VoiceWave.Distribution.channel` | `github` (existing, unchanged) |
| `VoiceWave.Speech.path`, `.Speech.modelFamily`, `.Gpu.vendor`, `.Os.major` | existing |
| `VoiceWave.Install.source` | `winget`, `scoop`, `direct`, `unknown` |
| `VoiceWave.Install.ageBucket` | `0d`, `1-6d`, `7-29d`, `30-89d`, `90d+`, `unknown` |
| `VoiceWave.Event.delayed` | `no`, `yes` (yes when sent from the outbox more than 1 hour after it was queued) |

`App.activeDaily` only also gets `VoiceWave.Consent.reliability`: `on` or `off`. That makes the reliability opt-in rate
readable from daily actives.

### 4.3 New events

| Event | Tier | Dedupe | Properties (allowed values) | Source |
|---|---|---|---|---|
| `App.firstLaunch` | basic | once per install (outbox marker `funnel.first-launch`) | base only. Fires only when the install is new (section 6) | TS, after `getSupportContext` |
| `Acquisition.sourceReported` | basic | once per install (`funnel.acquisition`) | `VoiceWave.Acquisition.answer`: `reddit`, `x`, `youtube`, `github`, `search`, `ai-assistant`, `alternativeto`, `saashub`, `devhunt`, `friend`, `winget`, `article`, `other`, `skipped` | Onboarding done step |
| `Consent.answered` | basic | once per consent version | `VoiceWave.Consent.choice`: `basic-and-reliability`, `basic-only` (never sent for "everything off") | Consent dialog or onboarding |
| `App.uncleanExit` | reliability | once per detected exit | `VoiceWave.Exit.kind`: `crash`, `after-panic`, `system-restart`; `VoiceWave.Exit.phase`: `startup`, `idle`, `dictating`; `VoiceWave.Exit.uptime`: `lt-1m`, `1-10m`, `10-60m`, `1-8h`, `8h+` | Rust `session.rs` at startup |
| `Update.failed` | reliability | once per (targetVersion, stage) per UTC day | `VoiceWave.Update.stage`: `download`, `verify`, `install`, `not-applied`; `VoiceWave.Update.errorKind`: `network`, `signature`, `disk`, `permission`, `other`; `VoiceWave.Update.targetVersion`: semver or `other` | TS catch in updater callers, plus Rust `not-applied` |
| `Hotkey.registerFailed` | reliability | once per session | `VoiceWave.Hotkey.failureKind`: `other-instance`, `hook-rejected`, `hook-thread`, `other` | Rust state.rs L2349 |
| `Insertion.targetDaily` | reliability | once per UTC day per (category, outcome) | `VoiceWave.Insert.targetCategory`: `browser`, `code-editor`, `terminal`, `chat`, `office`, `notes`, `ai-app`, `other`, `unknown`; `VoiceWave.Insert.outcome`: `typed`, `pasted`, `clipboard-only`, `history-only`, `blocked-elevated` | Rust insert.rs |

Notes on these events:
- **Volume check:** `Insertion.targetDaily` is capped at about 9 × 5 combinations per user per day, and in practice 1
  to 3. Sending one signal per dictation would cost tens of thousands of signals a month at the current 32 users.
- **Hotkey mapping:** in `state.rs`, the `"already owns the keyboard hook"` message maps to `other-instance`.
  `SetWindowsHookExW` and `GetModuleHandleW` failures map to `hook-rejected`. A `ready`/recv failure or a poisoned lock
  maps to `hook-thread`. Anything else maps to `other`.
  - User input errors (`EmptyBinding`, `InvalidToken`, `Unusable`, `Conflict`) are never sent.

### 4.4 Rust additions (`src-tauri/src/telemetry.rs`)

```rust
pub enum TelemetryEvent {
    DictationFailed { kind: DictationFailureKind, target: Option<InsertTargetCategory> }, // target only for kind=insert
    WorkerCrashed { worker: WorkerKind, exit: WorkerExit },
    Panicked { area: PanicArea },
    UncleanExit { kind: ExitKind, phase: ExitPhase, uptime: UptimeBucket },
    UpdateNotApplied { target: UpdateTarget },      // target = parsed semver or Other
    HotkeyRegisterFailed { kind: HotkeyFailureKind },
    InsertionTarget { category: InsertTargetCategory, outcome: InsertOutcome },
}
```

- Every new enum uses `#[derive(Serialize, Clone, Copy)]` and `#[serde(rename_all = "kebab-case")]`. The field
  serializes with the exact values from 4.3. Give `UptimeBucket` explicit `#[serde(rename = "lt-1m")]` style renames.
- `UpdateTarget` can be `{ major: u16, minor: u16, patch: u16 }`, formatted by TS. That keeps it closed.
- `funnelEventFromTelemetry` (telemetry.ts L400) maps each variant to its `FunnelEvent`. The tier comes from the type.

### 4.5 Insert target category (`src-tauri/src/insertion/mod.rs`)

Add this next to `terminal_paste_chord`:

```rust
pub enum InsertTargetCategory { Browser, CodeEditor, Terminal, Chat, Office, Notes, AiApp, Other, Unknown }

pub fn target_category(exe_name: Option<&str>) -> InsertTargetCategory {
    let Some(exe) = exe_name else { return InsertTargetCategory::Unknown };
    if is_terminal_process_exe(exe) { return InsertTargetCategory::Terminal; }
    match exe.to_ascii_lowercase().as_str() {
        "chrome.exe" | "msedge.exe" | "firefox.exe" | "brave.exe" | "opera.exe" | "vivaldi.exe" | "arc.exe" => Browser,
        "code.exe" | "code - insiders.exe" | "cursor.exe" | "windsurf.exe" | "zed.exe" | "devenv.exe"
        | "idea64.exe" | "pycharm64.exe" | "webstorm64.exe" | "rider64.exe" | "sublime_text.exe" | "notepad++.exe" => CodeEditor,
        "slack.exe" | "discord.exe" | "teams.exe" | "ms-teams.exe" | "whatsapp.exe" | "telegram.exe" | "signal.exe" => Chat,
        "winword.exe" | "excel.exe" | "powerpnt.exe" | "outlook.exe" | "olk.exe" | "onenote.exe" => Office,
        "notion.exe" | "obsidian.exe" | "notepad.exe" | "evernote.exe" | "logseq.exe" => Notes,
        "claude.exe" | "chatgpt.exe" | "codex.exe" => AiApp,
        _ => Other,
    }
}
```

- `InsertResult` (L28) gains `#[serde(default)] pub target_category: Option<InsertTargetCategory>`.
  - It is set where `insert_text` already resolves the foreground process.
  - If `insert_text` does not resolve it on every path, call `foreground_process_exe_name()` once at the start of the
    request, map it, and drop the string.
  - The category travels. The exe name never leaves the function.
- `InsertOutcome` mapping:
  - `blocked_by_elevation` maps to `blocked-elevated`.
  - Otherwise, `Direct` maps to `typed`, `ClipboardPaste` to `pasted`, `ClipboardOnly` to `clipboard-only` and
    `HistoryFallback` to `history-only`.
- `src-tauri/src/state/dictation/insert.rs`:
  - After a successful `insert_text_with_polish_meta` (about L130), call
    `telemetry::record(TelemetryEvent::InsertionTarget { .. })`.
  - At the failure record (L174), pass `target` when the kind is `Insert`.
  - Rust always records. The TS tier gate decides whether to send.

---

## 5. Outbox: persistence and retry (P2)

Replace "send now, drop on failure" with a persistent outbox in `src/lib/telemetry.ts`. The Rust queue stays as is: it
is drained into the outbox right away, and the outbox is the only place that sends.

**Storage:**
- localStorage key: `voicewave.analytics.outbox.v1`.
- WebView2 localStorage persists across launches and updates, which is the same store the install id already uses.

```ts
type OutboxEntry = {
  id: string;            // crypto.randomUUID(), for debugging only
  type: string;          // TelemetryDeck signal type
  tier: "basic" | "reliability";
  payload: Record<string, string>; // fully resolved at enqueue: base context + params
  createdAt: number;     // ms
  attempts: number;
  nextAt: number;        // ms, earliest next try
};
```

**Rules:**
1. **Enqueue** (`trackFunnelEvent` and drained Rust events):
   - Run the existing dedupe check.
   - If the event's tier is off, drop it.
   - Otherwise resolve the payload now (so the version and context reflect when it happened) and append.
   - Write the dedupe marker on enqueue, not on send. This stops duplicate enqueues across relaunches, and the outbox
     itself survives relaunches.
   - `once: "session"` markers stay in memory as today.
2. **Caps:**
   - At most 200 entries. When full, drop the oldest `basic` daily markers first, then the oldest entries overall.
   - At load, drop entries older than 14 days.
   - A single entry is never retried for more than 14 days.
3. **Flush** (single flight, using the existing `enqueue` promise chain at L110):
   - Process due entries oldest first.
   - Re-check the tier consent per entry at send time.
   - Before sending, set `VoiceWave.Event.delayed` to `yes` when `now - createdAt > 3600000`.
   - **Response handling:**

     | Response | Action |
     |---|---|
     | `ok` | Remove the entry |
     | HTTP 400, 401, 403, 413, 422 | Remove the entry. It can never succeed (same as T3's drop list) |
     | Network throw, 408, 429, 5xx | Set `attempts += 1` and `nextAt = now + min(2000 * 2^attempts, 300000)` with ±20% jitter. Stop this flush there to keep order |

   - Write the outbox to storage after every change. It is small, so a full rewrite is fine.
4. **Flush triggers:**
   - After every enqueue.
   - Once after settings load (useVoiceWave.ts drain effect, about L2519).
   - `window` `online` event.
   - A 60 s interval, only while the outbox is non-empty.
   - `document` `visibilitychange` to visible.
5. **Opt-out:**
   - Turning basic off clears the whole outbox.
   - Turning reliability off removes the `reliability` entries.
   - Hook this into the settings update path in useVoiceWave.ts. Call `purgeOutbox(tier)` from the same handler that
     saves the toggle.
6. **No app id configured** (`TELEMETRY_APP_ID` empty, which covers local builds): never enqueue.

**Tests to add in `src/lib/telemetry.test.ts`:**
- The outbox survives a simulated reload.
- Backoff timing (fake timers).
- 413 drops the entry.
- 503 keeps it.
- Opting out purges.
- Reliability events are dropped while reliability is off.
- `delayed=yes` after 1 h.
- The cap of 200.
- The dedupe marker is set on enqueue.

---

## 6. Install age and "new install" (P2)

**Rust**, in a new `src-tauri/src/install_info.rs`, or inside `support.rs`:
- On first launch, when `FirstSeenUtcMs` is absent:
  - If `settings.json` already exists (`proj_dirs.config_dir().join("settings.json")`, settings/mod.rs L320), this is a
    pre-0.7.7 install. Use that file's creation time (`std::fs::metadata(..).created()`), or the current time if that
    fails, and set `FirstSeenEstimated = 1`.
  - Otherwise this is a new install. Use the current time, set `FirstSeenEstimated = 0` and
    `FirstSeenVersion = CARGO_PKG_VERSION`.
- Run this check **before** settings are first saved in setup. Otherwise the file always exists.
  - The safest approach is to capture `settings_existed_at_launch: bool` in a `OnceLock` at the very start of setup.
- `is_new_install = !FirstSeenEstimated && FirstSeenVersion >= 0.7.7`. Expose it in `SupportContext` as
  `isNewInstall`.
- Bucket: `days = (now - first_seen) / 86_400_000`.

  | Days | Bucket |
  |---|---|
  | 0 | `0d` |
  | 1 to 6 | `1-6d` |
  | 7 to 29 | `7-29d` |
  | 30 to 89 | `30-89d` |
  | 90 or more | `90d+` |
  | Registry unreadable | `unknown` |

**TS:** `App.firstLaunch` is enqueued once after `getSupportContext()` resolves, and only if `isNewInstall` is true. The
dedupe marker makes it fire once.

---

## 7. Unclean exit and Update.failed (P3)

**New file:** `src-tauri/src/session.rs`, registered in `lib.rs` (`mod session;`).

**Session file:** `%LOCALAPPDATA%\voicewave\localcore\data\session.json`, written with `atomic_file::atomic_write`:

```json
{ "v": 1, "appVersion": "0.7.7", "pid": 1234, "startedAtMs": 0, "lastAliveMs": 0,
  "bootAtMs": 0, "phase": "startup", "updateTarget": null }
```

- `bootAtMs = now_ms - GetTickCount64()`, rounded to the minute. Two sessions in the same Windows boot share this value
  to within a few seconds.
- `phase` is one of `startup`, `idle`, `dictating`, `updating`.

**Startup**, in setup after the single-instance claim and after `support::init()`:
1. Read the previous file, if any. Then:
   - If its `pid` is alive and its image is VoiceWave, ignore it. This is defensive, because single instance should
     prevent it.
   - If `phase == "updating"`:
     - If `appVersion` equals the current version, record `UpdateNotApplied { target }`.
     - Otherwise the update worked, and nothing is sent.
   - Otherwise:
     - Kind is `after-panic` when the panic hook marked the file. Otherwise it is `system-restart` when
       `|prev.bootAtMs - current bootAtMs| > 120000`. Otherwise it is `crash`.
     - Uptime is `lastAliveMs - startedAtMs`, bucketed as `lt-1m`, `1-10m`, `10-60m`, `1-8h` or `8h+`.
     - Record `UncleanExit { kind, phase: prev.phase, uptime }`.
2. Write a fresh file with `phase: "startup"`, then `"idle"` once the controller is ready.

**Heartbeat:** a tokio interval every 5 min rewrites `lastAliveMs`.

**Phase:**
- Set `dictating` when capture starts (`state/dictation/capture.rs`) and `idle` when the session ends, in the same places
  that already emit state changes.
- These are two small atomic writes per dictation. If that feels heavy, write phase changes only from the heartbeat and
  at most once every 10 s.

**Panic:** in the `support.rs` panic hook (L96), after writing `panics.log`, call `session::mark_panicked()`. It is
best effort and must never panic itself.

**Clean exit:** in `RunEvent::Exit` (lib.rs L2654), call `session::clear()` (delete the file) next to
`shutdown_inference_workers()`.

**Updating:**
- Add the command `mark_update_installing(target_version: String)`. It validates the semver and writes
  `phase: "updating"` and `updateTarget`.
- Call it from `src/lib/updater.ts` `installPendingUpdate` before `downloadAndInstall`.
- On a thrown error, call `clear_update_installing()` so phase returns to `idle`.
- Alternative for the same goal: `on_before_exit` on the Rust `updater_builder()`, which runs right before the Windows
  installer is spawned. The JS plugin path used today does not expose it, so the command is simpler.

**`Update.failed` from TS:** wrap `downloadAndInstall` in `installPendingUpdate`.
- **Stage:**
  - No `Finished` event seen yet means `download`.
  - An error whose message contains `signature` or `minisign` (checked case-insensitively) means `verify`.
  - Anything after `Finished` means `install`.
- **errorKind** comes from the error text, matched locally and then discarded:

  | Error text contains | errorKind |
  |---|---|
  | `network`, `request`, `dns`, `timed out`, `connection` | `network` |
  | `signature` | `signature` |
  | `space`, `disk`, `os error 112` | `disk` |
  | `access is denied`, `os error 5` | `permission` |
  | anything else | `other` |

- Emit the event from the catch blocks in `UpdatePrompt.tsx` L94 and `UpdateSection.tsx` L72. Do it through a helper in
  updater.ts so both callers share it.
- `checkForUpdate` failures are **not** counted, because offline is normal.

**Unclean exit banner** (next launch, only for `crash` or `after-panic`, shown once, independent of consent): see the
copy in 8.6. Rust exposes `previous_exit_unclean: bool` in `SupportContext`.

**Tests (Rust, in `session.rs`):**
- Each branch: clean, crash, after-panic, system-restart, updating success, updating not applied.
- Uptime buckets.
- A corrupt file is treated as absent and nothing is sent.

---

## 8. Consent v2 (P2)

### 8.1 Settings and migration (`src-tauri/src/settings/mod.rs`)

- Add `#[serde(default)] pub reliability_reports_opt_in: bool`. Default `false` in both `Default` (L273) and serde, so
  nothing in that tier is sent before an explicit choice.
- Add `#[serde(default)] pub analytics_consent_prompt_shows: u32`.
- Change `ANALYTICS_CONSENT_VERSION` from 1 to 2 (L403), and the TS mirror in telemetry.ts L19.
- **Rewrite `migrate_analytics_consent` (L409).** The current code would silently mark every opted-in user as having
  seen v2, which skips the prompt.

```rust
fn migrate_analytics_consent(settings: &mut VoiceWaveSettings) {
    if !settings.onboarding_completed { return; } // onboarding asks
    if !settings.anonymous_usage_opt_in {
        // They said no before. Respect it: no new prompt, everything stays off.
        settings.reliability_reports_opt_in = false;
        settings.analytics_consent_version = ANALYTICS_CONSENT_VERSION;
        return;
    }
    // Opted in (v0 or v1): leave the version below 2 so the v2 prompt shows once.
}
```

- `src/types/voicewave.ts`: add `reliabilityReportsOptIn` and `analyticsConsentPromptShows` to the settings type, and to
  the fallback settings object in useVoiceWave.ts.
- The consent provider in useVoiceWave.ts L2495 becomes tier-aware:

```ts
consent: (tier) => settingsLoadedRef.current && settingsRef.current.anonymousUsageOptIn &&
  (tier === "basic" || settingsRef.current.reliabilityReportsOptIn)
```

### 8.2 Who sees what

| User | What they see |
|---|---|
| New install (onboarding) | Welcome step shows both toggles, both on by default **[HIS CALL]**. Choosing Continue or Skip saves both values and sets `analyticsConsentVersion = 2` |
| Existing, basic on, version < 2 | The v2 dialog, once, after the update (App.tsx `showAnalyticsConsent`, same gating as today: not during onboarding or the model gate) |
| Existing, basic off | Nothing. Migration sets version 2 and keeps everything off |
| Dialog not answered (app quit while open) | Shown again on the next launch, up to 3 times (`analyticsConsentPromptShows`). After that, version 2 is stored with basic kept and reliability off |

### 8.3 Decline behaviour **[HIS CALL]**

Proposal: "No" means **keep basic, problem reports off**. That is exactly what the user already accepted in v1, so
saying no to something new never takes anything away from them and never adds anything.
- "Turn everything off" is offered as a separate, equally visible button.
- The alternative, where "No" turns everything off, would cost most existing users' basic counts. Users who just want
  the dialog gone would click No.

### 8.4 Existing user dialog copy

Replace the dialog at App.tsx L3737 to L3765. All copy below has no dashes.

```text
Title:     A quick privacy check

Intro:     VoiceWave 0.7.7 can send a few more anonymous facts that help us fix problems.
           Nothing new is sent unless you say yes. You can change this anytime in Settings → Diagnostics.

Block 1 heading:  Basic usage count (you have this on today)
Block 1 body:     A random install ID, the app version, how you installed VoiceWave (winget, Scoop or a
                  direct download), roughly how long ago you installed it, setup progress, a daily active
                  marker, the type of a failed dictation or download, and which part of the app hit an
                  internal error. New in 0.7.7: the install method and install age.

Block 2 heading:  Problem reports (new)
Block 2 body:     When something goes wrong: that VoiceWave closed unexpectedly, that an update did not
                  install, or that the keyboard shortcut could not be set up. Also the kind of app your text
                  went into, such as a browser, a code editor or a chat app. Never the app's name.

Never line:       Never sent: audio, transcripts, window titles, app names, file paths, device names, error
                  messages, or your settings.

Button 1 (primary):    Yes, send problem reports too
Button 2 (secondary):  No, keep basic only
Button 3 (text):       Turn everything off
```

- Button 1 sets `reliability=true` and `version=2`, then sends `Consent.answered` with `choice=basic-and-reliability`.
- Button 2 sets `reliability=false` and `version=2`, then sends `Consent.answered` with `choice=basic-only`.
- Button 3 sets `basic=false`, `reliability=false`, `version=2`, purges the outbox and sends nothing.
- Give the dialog `aria-label="Privacy check"`.
- Escape and backdrop click do nothing, matching the current dialog. Focus starts on Button 2, the neutral choice.

### 8.5 Onboarding welcome step copy

This goes in `src/components/Onboarding.tsx`, in the existing toggle block of the welcome step. The props
`usageOptIn` and `onUsageOptInChange` (L50 to L54) gain siblings `reliabilityOptIn` and `onReliabilityOptInChange`.

```text
Toggle 1 title:  Anonymous usage count · on by default
Toggle 1 body:   Counts installs, how you installed, and setup progress with a random ID. Never audio,
                 transcripts, or app names.
Toggle 1 aria:   Share anonymous usage count

Toggle 2 title:  Problem reports · on by default
Toggle 2 body:   Tells us when VoiceWave closes unexpectedly, an update fails, or the shortcut cannot be set
                 up, plus the kind of app you dictate into. Never the app's name.
Toggle 2 aria:   Share problem reports

Details link (both):  What's sent
```

- Toggle 2 is disabled and unchecked while Toggle 1 is off.
- The `UsageCountDetails` component (L340) shows `ANONYMOUS_USAGE_DETAILS`. Add a sibling `ReliabilityDetails` that
  shows `RELIABILITY_DETAILS`.

### 8.6 Other copy

**Acquisition question.** Put it on the onboarding done step, below the success message. Show it only if
`isNewInstall` and basic is on.

```text
Heading:   One quick question
Prompt:    Where did you hear about VoiceWave?
Helper:    Optional. Your answer is sent once with the anonymous usage count.
Choices:   Reddit · X · YouTube · GitHub · Google or another search engine · ChatGPT or another AI
           assistant · AlternativeTo · SaaSHub · DevHunt · A friend or coworker · Browsing winget ·
           A blog or article · Somewhere else
Skip link: Skip
After pick: Thanks!
```

- The choice-to-value map runs in label order: `reddit`, `x`, `youtube`, `github`, `search`, `ai-assistant`,
  `alternativeto`, `saashub`, `devhunt`, `friend`, `winget`, `article`, `other`. Skip sends `skipped`.
- Use single-select chips (`role="radiogroup"`). Picking one sends right away.
- No free text field, ever.
- If the user leaves the done step without choosing, send `skipped` once so the response rate is measurable.
- It is never shown again (`funnel.acquisition` marker).

**Settings → Diagnostics** (App.tsx about L3111, where "Anonymous usage count (on by default)" is today):

```text
Toggle 1:  Anonymous usage count
           Counts installs, how you installed, and setup progress with a random ID.
Toggle 2:  Problem reports
           Tells us about unexpected closes, failed updates, shortcut problems, and the kind of app you
           dictate into. Needs the usage count to be on.

Support file heading:  Support file
Support file body:     Saves a zip file with your diagnostics report and recent logs to your Downloads
                       folder. Nothing is uploaded. Attach it to a GitHub issue if you want help.
Button:                Save support file
After save:            Saved to {path}
Button after save:     Show in folder
Error:                 Couldn't save the support file. You can still open the logs folder.
```

**Unclean exit banner** (home view, next launch, dismissible):

```text
VoiceWave closed unexpectedly last time. If it keeps happening, save a support file and attach it to a
GitHub issue.
[Save support file]  [Open logs folder]  [Dismiss]
```

**Update failure** (UpdatePrompt and UpdateSection error line):

```text
The update didn't install. VoiceWave will try again later, or you can download the latest version from
voicewave.site.
```

### 8.7 Disclosure constants (`src/lib/telemetry.ts`)

Replace `ANONYMOUS_USAGE_DETAILS` (L22) and add `RELIABILITY_DETAILS`:

```ts
export const ANONYMOUS_USAGE_DETAILS =
  "A random install ID, a random ID that changes every launch, the app version, how you installed VoiceWave " +
  "(winget, Scoop, a direct download, or unknown), roughly how long ago you installed it (in ranges such as " +
  "under a week or over 90 days), a first launch marker, a first use marker, a once a day active marker, a once " +
  "a day launch marker and whether the app started with Windows, setup progress, your answer to where you heard " +
  "about VoiceWave if you gave one, whether your first dictation ran on the graphics card or the processor, model " +
  "download failures (with a failure category and how far the download got), graphics card to processor " +
  "fallbacks, the type of a failed dictation (for example no microphone or a timeout), the type of a speech " +
  "helper crash, and which part of the app hit an internal error. Each comes with whether speech runs on the " +
  "graphics card or the processor, the speech model family, the graphics card maker, and Windows 10 or 11. " +
  "Never audio, transcripts, error messages, dictionary or snippet content, account identity, device names, " +
  "app names, window titles, filenames, or your other settings.";

export const RELIABILITY_DETAILS =
  "That VoiceWave closed unexpectedly last time (a crash, after an internal error, or a Windows restart), what " +
  "it was doing then (starting, idle, or dictating) and roughly how long it had been running; that an update " +
  "did not install and at which step; that the keyboard shortcut could not be set up and why, as a category; " +
  "and once a day, the kind of app your text went into (browser, code editor, terminal, chat, office, notes, " +
  "AI app, or other) and how it got there (typed, pasted, or left on the clipboard). The app's name is turned " +
  "into one of those kinds on your PC and never sent.";
```

Both strings were checked for dashes. Keep it that way. A test should assert that `/[-\u2013\u2014]/` does not match
any user-facing string in telemetry.ts.

---

## 9. Support zip and Open logs (P4)

**New command** `export_support_zip` in lib.rs, next to `open_logs_folder` (L1267). Register it in the handler list at
about L2578. Add a `saveSupportZip()` TS wrapper in `src/lib/tauri.ts` near L408, plus a `revealInFolder(path)` wrapper
that runs `explorer.exe /select,<path>` through `support::shell_open` or `Command`.

**Cargo:** `zip = { version = "2", default-features = false, features = ["deflate"] }`.

**Output:** `%USERPROFILE%\Downloads\voicewave-support-YYYYMMDD-HHMM.zip`. If Downloads cannot be resolved, it falls
back to the logs dir. Use the `directories::UserDirs` download dir.

**Contents:**

| Entry | Source | Redaction |
|---|---|---|
| `report.txt` | `diagnostics_report(...).text()` (same as Copy diagnostics) | Already redacted |
| `diagnostics.json` | Existing bundle builder (diagnostics/mod.rs L340), but **not gated** on `diagnostics_opt_in`, because this is local and user-initiated. If no metrics are recorded, it has an empty aggregate | Already metadata only |
| `logs/voicewave.log.YYYY-MM-DD` | The newest 3 daily log files, each cut to its last 2 MB | Each line passes through `support::redact_profile_dir` (L173) |
| `panics.log` | Copy, without draining (do not call `take_panic_areas`) | `redact_profile_dir` |
| `session.json` | The previous session file, if the last exit was unclean | No paths inside |
| `README.txt` | Fixed text: "This file was made by VoiceWave on your PC. It has no audio and no transcripts. Look through it before you share it." | |

**Rules:**
- Cap the zip at 8 MB.
- Never include `settings.json`, history, dictionary, snippets, models or the secure store.
- Before 0.7.7 ships, grep the logs for anything that should not be in them. At 9a214a1, no `tracing::` call in
  `insertion/` or `state/dictation/insert.rs` prints window titles or exe names.
  - Add a unit test that feeds a log line with `C:\Users\Alice\...` through the zip writer and checks for
    `%USERPROFILE%`.

**Open logs folder:** it exists already. Reuse `showLogsFolder` (App.tsx L1355) in two new places:
- The unclean exit banner.
- Next to "Save support file" in Settings → Diagnostics.

Keep the current button in Report a problem.

**Issue form:** `issue_report_url` (support.rs L420) stays as is. Add one line to the issue template body:
"If you saved a support file, drag it into this box."

---

## 10. UTM links, start now (P0, no release)

GoatCounter on voicewave.site reads `utm_campaign` (or `campaign`) as the campaign and `utm_source` (or `ref`, `src`,
`source`) as the source, with no setup (goatcounter.com/help/campaigns). `https://voicewave.site/?utm_source=test`
returns 200 with no redirect, so the parameters survive. Note that `www.voicewave.site` returns a 301, so always use the
apex domain.

Convention: all lowercase, no spaces, `utm_source` = where, `utm_medium` = type, `utm_campaign` = which push.

| Where | URL |
|---|---|
| r/windowsapps post, Thu Oct 29 | `https://voicewave.site/?utm_source=reddit&utm_medium=social&utm_campaign=windowsapps-2026-10-29` |
| Same post, your own follow-up comment | `https://voicewave.site/?utm_source=reddit&utm_medium=social&utm_campaign=windowsapps-2026-10-29&utm_content=comment` |
| AlternativeTo listing (website field) | `https://voicewave.site/?utm_source=alternativeto&utm_medium=directory&utm_campaign=listing` |
| SaaSHub listing | `https://voicewave.site/?utm_source=saashub&utm_medium=directory&utm_campaign=listing` |
| DevHunt launch | `https://voicewave.site/?utm_source=devhunt&utm_medium=launch&utm_campaign=devhunt-launch` |
| X profile bio | `https://voicewave.site/?utm_source=x&utm_medium=social&utm_campaign=profile` |
| X post (one per post, by date) | `https://voicewave.site/?utm_source=x&utm_medium=social&utm_campaign=post-2026-10-29` |

Notes:
- Link to the site, not straight to the .exe or GitHub release. The site's existing click events (`download-exe-hero`,
  `download-exe-final`, `github-hero`) then show what the visitor did.
- Reddit and X keep query strings. X shortens links to t.co, but the redirect keeps the query.
- Directory listings can be edited later. Updating an existing AlternativeTo or SaaSHub URL is an edit Rithwik makes
  himself.
- Optional, needs a repo edit: the GitHub README "website" link could carry
  `?utm_source=github&utm_medium=readme&utm_campaign=readme`.
- The in-app answer (`Acquisition.sourceReported`) and GoatCounter campaigns are separate funnels. GoatCounter shows
  visits and clicks, and TelemetryDeck shows installs and their self-reported source. There is no join between them,
  by design.

---

## 11. README and privacy notice text (P5)

### 11.1 README, replacing the "Anonymous usage count" section (README.md L104 to L126)

The bullets use `*` so the copy itself has no dashes.

```markdown
## Anonymous usage count and problem reports

VoiceWave can send two kinds of anonymous facts through [TelemetryDeck](https://telemetrydeck.com). You choose
during setup, and you can change either one anytime in **Settings → Diagnostics**.

**Anonymous usage count** (on by default for new installs since 0.6.1). Every event carries a random installation
ID, the app version, the download channel, how VoiceWave was installed (winget, Scoop, a direct download, or
unknown), roughly how long ago it was installed (in ranges), Windows 10 or 11, the GPU vendor (NVIDIA, AMD, Intel
or none), which speech model family is active, and whether it runs on the GPU or the CPU. It counts only:

* the first launch and your first successful dictation, and whether it ran on the GPU or the CPU;
* one "active today" marker per day, and one "launched today" marker (started with Windows or by hand);
* which setup step you reached, and whether setup finished or was skipped;
* where you heard about VoiceWave, if you picked an answer from the list during setup;
* a dictation failing, as a category such as "no speech heard" or "microphone missing";
* a speech or AI polish model download failing, with a failure category and how far it got, in quarter steps;
* the GPU engine falling back to the CPU, with a reason category;
* a speech worker crashing or timing out, or the app hitting an internal error, by area.

**Problem reports** (new in 0.7.7, asked once). These need the usage count to be on. They count only:

* VoiceWave closing unexpectedly, as a category (crash, after an internal error, or a Windows restart), what it
  was doing, and roughly how long it had run;
* an update not installing, and at which step;
* the keyboard shortcut not being set up, as a category;
* once a day, the kind of app your text went into (browser, code editor, terminal, chat, office, notes, AI app,
  or other) and whether it was typed, pasted or left on the clipboard. The app's name is turned into a kind on
  your PC and is never sent.

Neither one ever sends audio, transcript text, dictionary or snippet content, error messages, window titles, app
names, file paths, device names, account details or settings.

**How the install method is known.** The winget package passes a flag to the installer, and the installer saves
"winget" on your PC. A download from the website saves "direct". Anything else saves "unknown". Updates keep the
first value.

**Support file.** Settings → Diagnostics → Save support file writes a zip with your diagnostics report and recent
logs to your Downloads folder. It never leaves your PC unless you attach it to an issue yourself.
```

### 11.2 New `docs/PRIVACY.md`

```markdown
# VoiceWave privacy notice

VoiceWave runs speech recognition on your PC. Your audio and your text never leave it.

## What can be sent

Only if you leave them on, and only through TelemetryDeck:

* **Anonymous usage count:** the facts listed in the README under "Anonymous usage count".
* **Problem reports:** the facts listed in the README under "Problem reports".

Every value comes from a fixed list written into the app. Nothing you type, say or open is ever a value.

## What is never sent

Audio, transcripts, dictionary or snippet content, window titles, app names, file paths, device names, error
messages, account details, and settings.

## Identifiers

A random installation ID made on your PC. TelemetryDeck stores a hashed form of it. It is not linked to your
name, email or account. Delete it by turning the usage count off and reinstalling, or by deleting VoiceWave's app
data when you uninstall.

## Your choices

Settings → Diagnostics has one switch for each. Turning one off deletes anything waiting to be sent in that
group, right away.

## The website

voicewave.site uses GoatCounter, which counts page views without cookies. Links we share carry a tag that says
where the link was posted, such as reddit or x.

## Contact

Open an issue on GitHub.
```

### 11.3 Website FAQ, one paragraph

This replaces the usage count sentence in `voicewave-website/src/components/Faq.tsx` L12 to L13. It is a website repo
change and goes live on the next deploy.

```text
VoiceWave sends an anonymous usage count (a random install ID, the app version, how you installed it, and a few
setup events) and, if you agree, short problem reports such as an unexpected close or a failed update. Both go
through TelemetryDeck and each has its own switch. Never audio, transcripts, app names or file paths.
```

### 11.4 CHANGELOG entry for 0.7.7

```text
### Privacy and diagnostics
* New: an optional "Problem reports" switch (asked once) for unexpected closes, failed updates, shortcut setup
  problems, and the kind of app text goes into. Off unless you say yes.
* New: the usage count now includes how VoiceWave was installed and a rough install age.
* New: an optional "Where did you hear about VoiceWave?" question during setup.
* New: Settings → Diagnostics → Save support file.
* Fixed: usage events that failed to send are now kept and retried instead of being lost.
```

---

## 12. Exact file list

| File | Phase | Change |
|---|---|---|
| `src-tauri/windows/hooks.nsh` | P1 | `NSIS_HOOK_POSTINSTALL` (new), extend `NSIS_HOOK_POSTUNINSTALL` |
| `src-tauri/src/support.rs` | P1 P3 P4 | `install_source()`; the panic hook calls `session::mark_panicked()`; log redaction reused by the zip |
| `src-tauri/src/install_info.rs` (new) or support.rs | P2 | First seen, `is_new_install`, age bucket, registry helpers |
| `src-tauri/src/session.rs` (new) | P3 | Session file, unclean exit, updating phase, heartbeat |
| `src-tauri/src/telemetry.rs` | P3 | New enum variants and value enums (section 4.4) |
| `src-tauri/src/insertion/mod.rs` | P3 | `InsertTargetCategory`, `target_category()`, `InsertResult.target_category` |
| `src-tauri/src/state/dictation/insert.rs` | P3 | Record `InsertionTarget`; add `target` to `DictationFailed` for insert |
| `src-tauri/src/state/dictation/capture.rs` (and wherever a session ends) | P3 | Session phase `dictating` and `idle` |
| `src-tauri/src/state.rs` L2349 to L2370 | P3 | Record `HotkeyRegisterFailed { kind }` |
| `src-tauri/src/settings/mod.rs` | P2 | `reliability_reports_opt_in`, `analytics_consent_prompt_shows`, version 2, new migration |
| `src-tauri/src/diagnostics/mod.rs` | P4 | Expose a non-gated bundle builder for the zip (keep `export_bundle`'s gate) |
| `src-tauri/src/lib.rs` | P1 to P4 | `SupportContext` fields; `mod session`; startup check; heartbeat; `RunEvent::Exit` clear; commands `mark_update_installing`, `clear_update_installing`, `export_support_zip`, `reveal_in_folder`; handler registration |
| `src-tauri/Cargo.toml` | P4 | `zip` crate |
| `src/lib/telemetry.ts` | P2 P3 | Outbox, tiers, new `FunnelEvent` members and guards, constants, version 2 |
| `src/lib/updater.ts` | P3 | `mark_update_installing` call, failure classify helper |
| `src/lib/tauri.ts` | P3 P4 | Wrappers for the new commands |
| `src/types/voicewave.ts` | P1 to P3 | Settings and `SupportContext` fields, new event types |
| `src/hooks/useVoiceWave.ts` | P2 P3 | Tier-aware consent provider, context properties, outbox flush triggers, `App.firstLaunch`, purge on toggle |
| `src/App.tsx` | P2 to P4 | Consent v2 dialog, Settings toggles, support file block, unclean exit banner |
| `src/components/Onboarding.tsx` | P2 P3 | Second toggle, `ReliabilityDetails`, acquisition question on the done step |
| `src/components/UpdatePrompt.tsx`, `src/components/UpdateSection.tsx` | P3 | Send `Update.failed`; new error copy |
| `.github/workflows/release.yml`, `.github/workflows/winget-release.yml` | P1 | Manifest switch guard step |
| `README.md`, `docs/PRIVACY.md` (new), `CHANGELOG.md` | P5 | Section 11 |
| `voicewave-website/src/components/Faq.tsx` | P5 | Section 11.3 |
| Tests: `src/lib/telemetry.test.ts`, `src/components/Onboarding.test.tsx`, `src/App.test.tsx`, `src/lib/updater.test.ts`, Rust tests in each changed module | all | Section 13 |

---

## 13. Test checklist

**Installer and install source** (on a clean Windows VM or Windows Sandbox):
- [ ] `setup.exe` run by double click: registry `InstallSource = direct`.
- [ ] `setup.exe /S`: `unknown`.
- [ ] `setup.exe /S /VWSOURCE=winget`: `winget`.
- [ ] `setup.exe /S /VWSOURCE=evil`: `unknown` (allowlist).
- [ ] Install `direct`, then run `/S /VWSOURCE=winget`: it stays `direct`.
- [ ] Install `winget`, then do an in-app update to a test build: it stays `winget`.
- [ ] Pre-0.7.7 install, then an in-app update to 0.7.7: no value, and the app reports `unknown`.
- [ ] Local winget test with the new manifest (`winget install --manifest <dir>`): `winget`.
- [ ] `winget install --manifest <dir> --override "/S"`: `unknown`.
- [ ] Uninstall without "Delete app data": the key stays. Uninstall with it: the key is gone.
- [ ] Check the generated `target/release/nsis/x64/installer.nsi` contains the hook and that `makensis` gives no
  warnings for it.

**Consent:**
- [ ] Existing user with basic on: the dialog appears once after the update.
  - [ ] Button 1 gives both on.
  - [ ] Button 2 gives basic only.
  - [ ] Button 3 gives all off with an empty outbox.
- [ ] Existing user with basic off: no dialog, version 2, nothing sent.
- [ ] Quitting with the dialog open shows it again next launch. After 3 shows it resolves to basic only.
- [ ] New install: both toggles show and default on. Turning the first off disables the second. Skip saves the
  values.
- [ ] Turning reliability off in Settings removes the queued reliability entries from localStorage.
- [ ] No reliability event appears in TelemetryDeck (test mode) while reliability is off.

**Events** (TelemetryDeck test mode via a dev build with an app id, or a staging app id):
- [ ] `App.firstLaunch` fires once on a new install and never on an existing one.
- [ ] Acquisition: choosing a chip sends that value once. Leaving sends `skipped` once. It never shows again.
- [ ] Unclean exit, crash: kill `voicewave` in Task Manager, relaunch, and get `crash` with the phase and uptime.
- [ ] Unclean exit, after-panic: a debug-only panic trigger.
- [ ] Unclean exit, system-restart: reboot with the app running (Windows may not deliver Exit).
- [ ] A clean tray Quit sends no event.
- [ ] Update.failed:
  - [ ] Point the updater at a bad signature: `verify`.
  - [ ] Pull the network mid-download: `download` / `network`.
  - [ ] Make the installer fail (read-only install dir), relaunch on the old version: `not-applied`.
- [ ] Hotkey.registerFailed: run two instances with single instance bypassed in dev, and get `other-instance` once.
- [ ] Insertion.targetDaily: dictate into Chrome, VS Code, Windows Terminal, Notepad, and an elevated Notepad. Each
  category and outcome is sent once per day, and the raw exe name appears nowhere in the payload.
- [ ] Every new payload validates against the allowed lists. Add a test that runs each event through
  `describeFunnelEvent` with junk input and expects `other`.

**Outbox:**
- [ ] Go offline, trigger 5 events, quit, relaunch online: all 5 are sent with `delayed=yes` when older than 1 h.
- [ ] A mocked 413 response is dropped, and a 503 is retried with backoff (fake timers).
- [ ] The cap of 200 holds, and entries older than 14 days are dropped at load.

**Support zip:**
- [ ] Contains `report.txt`, `diagnostics.json`, up to 3 logs, `panics.log`, `README.txt`, and nothing else.
- [ ] `rg -i "users\\\\<yourname>"` over the extracted files finds nothing.
- [ ] Size is at most 8 MB. Show in folder selects the file.
- [ ] Works with diagnostics opt-in off.

**Copy:**
- [ ] A test asserts that no user-facing string added in this release contains `-`, `–` or `—`.
- [ ] README, PRIVACY, FAQ and changelog are reviewed by eye for dashes. Markdown bullets use `*`.

**Regression:**
- [ ] The existing 0.7.6 events still arrive with the same names and property values (compare one day in
  TelemetryDeck before and after).
- [ ] `npm test`, `cargo test --features desktop`, `cargo clippy`.

---

## 14. Open questions for Rithwik

1. **[HIS CALL] Decline default.** Proposed: "No" keeps basic and leaves problem reports off, with a separate "Turn
   everything off". Alternative: "No" turns everything off.
2. **[HIS CALL] New users, problem reports default.** Proposed: on, shown on the welcome step next to the usage count,
   the same pattern as 0.6.1. Alternative: off for new users too.
3. **[HIS CALL] Ask users who opted out before?** Proposed: no, respect the earlier no and never prompt again.
4. **Tier for install source and install age.** Proposed: basic, disclosed in the v2 dialog as "New in 0.7.7". If you
   would rather keep basic identical to v1, move both to reliability.
5. **Acquisition choice list.** Add or drop any options (Product Hunt, Hacker News, Discord)? The values are fixed at
   release, and changing them later needs a new release.
6. **Insertion category list.** Is the exe to category map in 4.5 the right set? Unknown apps fall into `other`, which
   is safe but less useful.
7. **damn-good-b0t.** If it submits a VoiceWave version again without the switch, do you want to close its PRs, or ask
   its owner to carry Custom switches? Either is a manual external action.
8. **Support zip location.** Downloads (proposed) or the logs folder? Downloads is easier to find when attaching to an
   issue.
9. **Website FAQ and README UTM link.** Both need a repo edit and a deploy. Ship them with 0.7.7 or separately?

## 15. Sources

* winget schema 1.12.0 installer manifest, `InstallerSwitches.Custom`:
  https://github.com/microsoft/winget-pkgs/blob/master/doc/manifest/schema/1.12.0/installer.md
* winget-cli issue #239 (Custom passed in every mode; `--override` drops manifest switches):
  https://github.com/microsoft/winget-cli/issues/239
* komac releases (v2.12.0: "Fix installer switches not being carried over from the previous manifest"):
  https://github.com/russellbanks/Komac/releases
* winget-releaser updates manifests through komac: https://github.com/vedantmgoyal9/winget-releaser
* Tauri NSIS template (FileFunc include, `/UPDATE` and `/P` parsing, hook insertion points, uninstall data deletion):
  https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi
* Tauri updater, Windows `on_before_exit` and `installerArgs`: https://v2.tauri.app/plugin/updater/
* GoatCounter campaigns: https://www.goatcounter.com/help/campaigns
* winget-pkgs VoiceWave.LocalCore PRs: #462677 (0.7.6, rithwik1510), #461240 (0.7.5, rithwik1510), #455196 (0.7.0,
  damn-good-b0t); the 0.7.0 manifest is "Created with winmatsch".
