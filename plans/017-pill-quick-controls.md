# Plan 017: Pill Quick Controls

## Status

**Internal V1 implementation in progress (2026-08-12) — release remains gated
on the remaining Windows desktop matrix.**

### Phase 0 implementation checkpoint (2026-08-12)

- An internal, off-by-default spike is implemented behind
  `VOICEWAVE_PILL_QUICK_CONTROLS_SPIKE=1`.
- The spike tested the simpler Option B mouse path and a focused tray entry
  that captures and conditionally restores the prior foreground HWND.
- The global `WH_MOUSE_LL` hook remains rejected unless later evidence
  establishes a need that outweighs its system-wide cost.
- Frontend component/bridge tests and the production frontend build pass.
- Two live desktop runs failed the Option B mouse gate: the enlarged idle target
  produced a thick gray capsule after collapse, direct click did not open the
  WebView reliably, and the experimental geometry initially clipped the normal
  listening pill. **Option B is rejected.** The original thin, click-through
  `200 × 96` status window is restored for idle, listening, transcription, and
  post-transcription states. The spike card is tray-focused only.
- **The full V1 gate remains closed:** validate the tray-focused card, focus
  restoration, and ordinary dictation regression matrix before Phase 1–3. A
  future direct-click attempt requires a dedicated native hit-target design;
  it must not reuse this failed WebView/CSS coupling.
- Follow-up implementation now uses that dedicated design: the original visual
  pill remains unchanged and click-through, while a separate `72 × 20`
  Win32 layered hit-target window handles only the initial click with
  `WS_EX_NOACTIVATE` and `WM_MOUSEACTIVATE → MA_NOACTIVATE`. It is hidden for
  listening, transcribing, review, notices, and while the card is open. This
  implementation now passes the live click, cursor, expansion, and stable-hover
  checks. It remains internal until focus restoration, transparent-margin
  clicks, subsequent insertion targeting, DPI, fullscreen, and touch are
  validated.
- Dictionary correction review is now event-driven. The backend emits
  `voicewave://dictionary-queue-changed` only after candidates persist, and the
  pill reloads the queue from that completion event instead of racing fixed
  420/980ms timers.
- The inert Phase 0 proof button has been replaced behind the same internal
  flag with the approved V1 controls: four profiles, Start hands-free, Open
  VoiceWave, and an explicit Close button. The legacy whole-pill double-click
  route to the main window was removed because it conflicted with closing the
  card. Profile mutations use `set_dictation_profile` and emit the freshly
  persisted settings snapshot to both webviews.
- Phase 1–2 arbitration is now implemented internally: a pure typed display
  resolver owns the documented priority order, recovery is an explicit payload
  field rather than inferred from severity, and one `set_pill_window_mode`
  command owns compact/review/notice geometry. Quick Controls remains a native
  owner while open, so stale resize commands are ignored instead of racing it.
- Phase 4 and key desktop restraints are now implemented internally: focused
  tray entry closes on Escape or debounced native focus loss without globally
  consuming Escape; only the mouse path uses the 18-second timeout; profiles
  expose radio/arrow-key behavior and 36px minimum targets; feedback may wrap;
  reduced motion covers the interactive card; onboarding conditionally teaches
  the clickable pill only while the rollout is enabled.
- The separate native hit target now suppresses itself over borderless
  fullscreen foreground windows and re-evaluates every 400ms, while ordinary
  framed maximized windows stay interactive. `WM_POINTERUP` touch input is
  accepted and de-duplicated against Windows' promoted mouse event.
- The dedicated global shortcut remains the one unresolved V1 product choice.
  The current safest proposed locked default is `Ctrl+Alt+Shift+Q`; it is not
  added until that user-visible system-wide chord is approved.

This document defines a product and implementation proposal for turning the
existing bottom-center VoiceWave pill into a restrained, contextual control
surface. The product boundaries and formerly open decisions are resolved (see
"Resolved decisions"); no production code beyond the Phase 0 spike may be
written until the spike's acceptance criteria pass.

- **Priority:** Candidate P1
- **Effort:** M for V1, excluding later recovery/action expansions
- **Risk:** Medium–high desktop UX risk (focus theft, click interception,
  over-prompting)
- **Depends on:** Existing floating-pill window and Plan 003 typed actions
- **Planned against:** VoiceWave v0.6.0, commit `1fa4f71`
- **Revised:** 2026-08-12 — review pass folded in. Open decisions resolved
  (see "Resolved decisions"), Theme cut from V1, two-path invocation contract
  adopted, edge cases added to rules and smoke matrix.

---

## Summary

VoiceWave already places a small, animated pill at the bottom of the screen
during dictation. It communicates listening, transcribing, insertion, errors,
notices, dictionary review, and hands-free state. The same surface can remove
routine trips into the full application by expanding upward into a small Quick
Controls card.

The proposal is deliberately narrower than a miniature VoiceWave app:

> **The pill reports immediate state and offers immediate actions. The main app
> remains the place for management, history, and advanced configuration.**

The pill has two personalities:

1. **Status surface:** calm, glanceable, normally click-through.
2. **Action surface:** interactive only after explicit user intent or when one
   important recovery decision is required.

The primary V1 interaction is user-invoked. VoiceWave must not turn the pill
into a proactive pop-up assistant.

---

## Product thesis

The pill is better positioned than a settings page for controls that matter at
the moment of dictation:

- Which formatting profile will the next dictation use?
- Should the next dictation be hands-free?
- How can the full app be opened quickly?
- If the last operation failed, what single action recovers it?

This is valuable because the surface is already present where the user is
working. It is not valuable if it repeatedly interrupts work, guesses intent,
or duplicates the whole application.

### Product principles

1. **User-opened by default.** General controls appear only after an explicit
   click or dedicated Quick Controls shortcut.
2. **System-opened only for recovery.** Automatic expansion requires a current,
   actionable problem or a very high-confidence one-shot action.
3. **One question at a time.** The pill never stacks prompts or becomes a
   notification inbox.
4. **No focus theft.** Merely showing or expanding the pill must not redirect
   typing or change the remembered insertion target.
5. **Stable anchor.** The bottom-center anchor does not jump; the surface grows
   upward.
6. **Predictable profiles.** The pill changes the profile only when the user
   chooses one. It does not infer tone from the focused application.
7. **Fast exit.** Outside click, Escape (focused entry path only), starting
   dictation, or completing an action collapses the panel according to
   explicit rules below.
8. **Safe fallback.** A pill-window or control failure must never block
   dictation, transcription, insertion, or opening the main app.

---

## Current implementation reality

The foundation already exists, but Quick Controls is not a CSS-only change.

### Existing capabilities

- `src/components/FloatingPill.tsx`
  - Visual states: idle, listening, transcribing, inserted, error.
  - Dictionary review panel.
  - Runtime notices and rescue-transcript notices.
  - Typed one-tap actions for `copyTranscript` and `addDictionaryTerm`.
  - Active polish-profile glyph and hands-free indicator.
- `src/pill.css`
  - Compact pill animation.
  - Review surface (`396 × 92` inside a `500 × 160` window).
  - Notice surface (`420 × 64` inside a `560 × 180` window).
  - Rescue surface (`460 × 96`).
  - Content enters after the surface starts expanding.
- `src-tauri/src/lib.rs`
  - A separate transparent, undecorated, always-on-top, taskbar-hidden pill
    window.
  - Runtime window resizing and bottom-center positioning.
  - Click-through control using `set_ignore_cursor_events`.
  - Separate compact, review, and notice window geometries.
- `src-tauri/src/state.rs`
  - `voicewave://pill-notice` event channel.
  - Typed `PillAction` payloads.
  - Recovery and dictionary-suggestion emitters.

### Critical constraint: the idle pill is currently click-through

The compact OS window is `200 × 96`, intentionally larger than the visible
idle surface so shadows are not clipped. Listening and collapsed modes call
`set_ignore_cursor_events(true)`. Therefore the visible idle pill cannot simply
receive an `onClick` handler today.

Making the entire transparent `200 × 96` always-on-top window interactive
would steal clicks from the application underneath. Clicking a conventional
webview window may also activate VoiceWave and destroy the original foreground
target.

**This is the first engineering gate, not an implementation detail to defer.**

### Second constraint: keyboard access and "no focus theft" are mutually exclusive

A non-activating window (`WS_EX_NOACTIVATE`-style) receives mouse clicks
without stealing focus, but it can **never** receive Tab, arrow keys, Space,
or Escape — keystrokes go to the focused application, which is deliberately
not the pill. Screen readers likewise cannot reach a window that cannot take
focus. Full keyboard operability and a strictly non-activating card cannot
both be true for the same entry path.

Resolution — a **two-path invocation contract** (see the invocation gate):

- **Mouse path:** non-activating click interaction on the visible pill.
- **Keyboard/accessibility path:** a dedicated shortcut and tray entry that
  *intentionally* focuses the pill window, remembers the previously focused
  HWND, and restores it on collapse.

The shortcut/tray entry is therefore a **V1 requirement, not a fallback** —
it is the only route for keyboard-only and screen-reader users, and the only
entry point when `showFloatingHud` is disabled.

---

## Proposed visual hierarchy

| Level | Shape | Typical size | Purpose |
|---|---|---:|---|
| Resting | Thin line / pill | Existing compact surface | Available, unobtrusive |
| Active | Wider capsule | Existing listening/transcribing sizes | Recording lifecycle |
| Notice | Long capsule | Existing notice/rescue sizes | Short information or one action |
| Quick Controls | Rounded card growing upward | Target ~340 × 150 surface | User-requested controls |

The Quick Controls surface should be a rounded rectangle, not a sharp square.
Suggested radius: 24–28px. It should visually emerge from the pill while being
large enough for real controls and accessible pointer targets.

### Motion

1. Lift the compact pill 8–12px.
2. Expand width and height from the bottom-center anchor.
3. Fade/translate content in after approximately 100–140ms.
4. Reverse that order when collapsing.
5. Target 220–300ms total animation.
6. Honor `prefers-reduced-motion` with opacity and near-instant geometry.
7. Do not slide laterally or move the bottom anchor during the transition.

Exact dimensions and timing require visual smoke testing at 100%, 125%, 150%,
and 200% Windows scaling.

---

## State model and priority

Introduce an explicit display-mode model rather than accumulating overlapping
booleans:

```ts
type PillDisplayMode =
  | { kind: "compact" }
  | { kind: "recording" }
  | { kind: "transcribing" }
  | { kind: "quickControls" }
  | { kind: "review"; itemId: string }
  | { kind: "notice"; noticeId: number }
  | { kind: "recovery"; noticeId: number };
```

The final representation may use a reducer or derived state, but it must encode
one active owner of the pill surface.

### Priority order

1. Blocking recovery/error
2. Active recording
3. Active transcription
4. Explicitly opened Quick Controls
5. Dictionary review
6. Post-dictation actionable notice
7. Read-only notice
8. Compact idle

### Arbitration rules

- Starting dictation closes Quick Controls immediately and shows Listening.
- Recording and transcription never display general settings.
- A recovery event may replace an inserted/error animation.
- A non-blocking notice arriving during recording is queued or dropped; it
  never covers the recording state.
- Quick Controls cannot open while a dictation is actively transcribing.
- Dictionary review and notices cannot own the surface simultaneously.
- Opening Quick Controls while a dictionary review panel is visible returns
  the review item to the queue; it re-emits on the next eligible idle. The
  two never stack.
- Only one queued notice may survive; newer higher-severity recovery replaces
  older informational content.
- Changing a profile while idle affects the next dictation only.
- V1 does not permit profile changes halfway through a recording.

---

## V1 Quick Controls

### Proposed layout

```text
┌────────────────────────────────────┐
│ VoiceWave                  Open ↗  │
│                                    │
│ Standard  Writing  Coding  Literal │
│                                    │
│        Start hands-free            │
└────────────────────────────────────┘
```

`Start hands-free` is the primary visual action — it is the only control on
the card that *starts* something; profiles are modifiers.

### Included in V1

1. Open/collapse Quick Controls from the pill click **and** from the
   dedicated shortcut/tray entry (both paths required; see invocation gate).
2. Show and select Standard, Writing, Coding, or Literal.
3. Start a hands-free dictation.
4. Open the full VoiceWave application.
5. Display local success/failure feedback for a setting change.
6. Collapse on the rules defined below.

### Explicitly excluded from V1

- Theme switching (cut 2026-08-12: a set-once setting with no
  moment-of-dictation value — the card's own inclusion bar — and inside a
  theme-invariant pill the selection produces no visible feedback; moved to
  follow-up candidates).
- Microphone dropdown.
- Audio-device diagnostics.
- Full transcript preview.
- History browsing.
- Dictionary or snippet management.
- Model download/install/remove controls.
- Account, billing, or authentication.
- Statistics.
- Advanced decode, VAD, release-tail, code-mode, or insertion settings.
- User-authored per-app rules.
- Selected-text command mode.
- Multiple simultaneous prompts.

The microphone picker is intentionally deferred. Native device refresh,
default-device semantics, long device names, and focus behavior make it a
separate interaction rather than a free extra row.

---

## Invocation design gate

Decision (2026-08-12): pursue **Option A for the mouse path**, with the
shortcut/tray focused entry shipping alongside it as the keyboard and
accessibility path regardless of the spike outcome. Option C becomes the
*only* path solely if Options A and B both fail the gate. Run the
Windows-native interaction spike before implementing the full card.

### Option A — no-activate interactive pill region (preferred)

- Keep the pill window non-activating using the appropriate Windows extended
  window style/behavior.
- Permit mouse interaction without making VoiceWave the foreground typing
  target.
- Make only the visible compact hit area consume clicks; transparent margins
  must pass through to the underlying application.
- Preserve the previously focused HWND as the insertion target.

This is the best user experience but may require Windows hit-testing beyond
Tauri's cross-platform API.

**WebView2 caveat:** input is received by WebView2's own child HWND
(`Chrome_WidgetWin_*`), not the outer Tauri window. Any `WM_NCHITTEST`
subclassing must target (or forward through) the child window — subclassing
only the outer HWND is the classic Tauri-on-Windows trap and will appear to
work while doing nothing. The spike must verify against the child window
explicitly.

**No focus signal:** because the Option A card never takes focus, there is no
blur event for outside-click detection. Outside-click collapse requires a
low-level mouse hook (`WH_MOUSE_LL`) — a new global hook with
antivirus-flagging and latency-budget implications the spike must evaluate —
or the idle auto-collapse timer as the fallback (see closing rules).

### Option B — shrink the interactive compact OS window

- Resize the idle window close to the visible hit target before enabling
  pointer events.
- Accept reduced shadow room in the idle state.
- Expand the window before animating Quick Controls.

This is simpler but creates a small always-on-top click interception area and
must be tested near browser controls, taskbars, full-screen apps, and remote
desktop sessions.

### Shortcut/tray entry — required V1 companion path (not a fallback)

- Open Quick Controls from a dedicated shortcut or tray action.
- This entry *intentionally* focuses the pill window: remember the previously
  focused HWND before taking focus, restore it on collapse.
- This is the only route for keyboard-only and screen-reader users, and the
  entry point when `showFloatingHud` is disabled.
- Note: taking focus deselects text in the target application. Harmless for
  V1's controls, but it poisons the future selected-text command mode
  (follow-up #6) — that feature must capture the selection *before* focusing
  or use the no-activate path.
- If Options A and B both fail the gate, this becomes the only entry
  (formerly "Option C") and the idle pill stays click-through.

### Mandatory spike acceptance criteria

- Clicking the visible pill opens Quick Controls.
- Clicking transparent pixels does not block the underlying application.
- Opening/using the panel does not alter the remembered insertion target.
- A subsequent dictation inserts into the application that was active before
  the panel opened, unless the user intentionally focuses another application.
- The panel works over normal, maximized, multi-monitor, and high-DPI windows.
- Hit-testing is verified against the WebView2 child HWND, not only the outer
  Tauri window.
- `WH_MOUSE_LL` outside-click detection is measured for cost and stability,
  or explicitly rejected in favor of the auto-collapse timer.
- A touch tap on the pill behaves like a mouse click (WM_POINTER activation
  rules differ from mouse for no-activate windows).
- If these cannot be proven, stop before building the rest of V1 and make the
  shortcut/tray entry the only path explicitly.

---

## Interaction rules

### Opening

- Primary: explicit click on the visible idle pill (no-activate mouse path).
- Required secondary entry: the dedicated Quick Controls shortcut and tray
  action (focused keyboard/accessibility path).
- Do not open on hover alone.
- Do not open because VoiceWave inferred that a different profile might help.

### Fullscreen policy

When the foreground application is fullscreen-exclusive (games, fullscreen
video), the pill must remain click-through or hidden. An interactive
always-on-top region at bottom-center is a misclick magnet in exactly the
spot many games place abilities and chat. Detect fullscreen-exclusive
foreground and suppress pill interactivity for its duration.

### Discoverability

Quick Controls never opens automatically and the restraint rules ban
post-dictation feature education — so nothing in normal use ever tells users
the pill became clickable. The one legitimate teaching slot is onboarding:
add a short beat to the existing rehearsal step showing the pill opening.
Without this, the feature ships as a secret.

### Closing

- Click outside the card: collapse (requires the `WH_MOUSE_LL` hook in the
  mouse path; if the spike rejects the hook, rely on the auto-collapse timer,
  dictation start, and the pill click acting as a toggle).
- Escape: collapses the card **only in the focused (shortcut/tray) path**,
  where the window legitimately receives keystrokes. In the no-activate mouse
  path the card cannot hear Escape, and it must NOT be delivered via the
  global keyboard hook — that would swallow Escape from the foreground app
  and collide with the Escape-cancel dictation behavior.
- Idle auto-collapse: ~15–20 seconds without interaction, as the safety net
  for the mouse path.
- Starting any dictation: collapse into the relevant recording state.
- Selecting a profile: keep the card open and confirm locally, allowing
  another selection.
- Starting hands-free: collapse immediately into Listening.
- `Open VoiceWave`: collapse after the main window is shown.
- Focused (shortcut/tray) path only: another app gaining focus collapses the
  card after a short debounce, unless doing so would swallow a control click.
  This rule is meaningless in the mouse path — the card never had focus, so
  no focus-loss event exists.

### Escape precedence

- Quick Controls open (focused path), no dictation: close Quick Controls.
- Listening: retain current VoiceWave behavior and cancel dictation.
- Transcribing: do not reinterpret Escape as a settings-panel command.
- Recovery notice: dismiss only when the underlying user content is safely
  recoverable elsewhere; a rescue transcript must not be discarded silently.

### Pointer and keyboard behavior

- Minimum interactive target: 36px; target 40px where geometry permits.
- Provide visible focus states; current global pill CSS removes outlines and
  must be corrected for interactive modes.
- Buttons use native semantics and meaningful accessible names.
- Arrow-key behavior is allowed for segmented controls only if implemented as
  a proper radiogroup; otherwise use Tab and Space/Enter.
- Keyboard operability and screen-reader access apply to the focused
  (shortcut/tray) entry path; the no-activate mouse path is mouse/touch only
  by construction.
- Do not add drag behavior to the Quick Controls card in V1.

---

## Automatic expansion policy

Quick Controls is user-opened. Separate recovery/action surfaces may still
open automatically under strict rules.

### Allowed automatic expansions

- No microphone signal → `Choose microphone` / `Retry` (future action).
- Model download failed → `Retry` (future action).
- Text insertion failed → existing rescue transcript + `Copy`.
- Microphone volume is unusably low → a direct recovery action if safe.
- Update is downloaded and ready → `Restart and update` / `Later` (future).
- One high-confidence dictionary candidate → existing gated Add action.

### Prohibited automatic expansions

- Theme suggestions.
- Profile suggestions based on application or content.
- Product tips or feature education after routine dictations.
- Marketing, release promotion, upgrade prompts, or streak reminders.
- Low-confidence dictionary candidates.
- Generic successful-insertion confirmation cards.

Every new automatic action requires:

1. A concrete user problem it resolves now.
2. A confidence/eligibility gate.
3. A cooldown or one-shot lifecycle.
4. A dismissal path.
5. Proof it does not double-surface the same issue in another UI.

---

## Settings and command architecture

### Do not submit a stale full settings object from the pill

The main window and pill are separate webviews. If both load a complete
`VoiceWaveSettings` snapshot and later save it, one can overwrite changes made
by the other.

Use narrow backend commands that lock, mutate, normalize, persist, and return
the fresh settings snapshot:

- Reuse `set_dictation_profile` for profile selection if its current contract
  is sufficient.
- Reuse the existing hands-free start/toggle command rather than simulating
  keyboard input.
- Reuse `show_main_window` for the Open action.

After a successful mutation, emit or reuse a settings-changed event so both
webviews reconcile. Do not introduce polling.

### Proposed bridge surface

Names are illustrative and must be reconciled with current conventions:

```ts
getPillQuickControlsSnapshot(): Promise<PillQuickControlsSnapshot>
setDictationProfile(profile: PolishProfile): Promise<VoiceWaveSettings>
startHandsFreeDictation(): Promise<void>
showMainWindow(): Promise<void>
setPillQuickControlsMode(open: boolean): Promise<void>
```

```ts
interface PillQuickControlsSnapshot {
  profile: PolishProfile;
  canStartHandsFree: boolean;
  disabledReason: string | null;
}
```

Keep the snapshot intentionally small. The pill does not need the entire app
settings model.

---

## Proposed implementation phases

### Phase 0 — Windows interaction spike

Files likely involved:

- `src-tauri/src/lib.rs`
- A small Windows-only window-style/hit-test helper if required
- `src/components/FloatingPill.tsx`
- `src/pill.css`

Deliverable:

- A temporary minimal panel with one inert button.
- Evidence for click-through outside the visible pill.
- Evidence for no focus theft and correct subsequent insertion target.
- Hit-testing verified against the WebView2 child HWND.
- `WH_MOUSE_LL` outside-click feasibility measured (cost, stability), or the
  auto-collapse-timer fallback chosen explicitly.
- Touch-tap behavior checked on a touch-capable device.
- A documented choice: Option A or B for the mouse path, or shortcut/tray as
  the only entry.

**STOP if the target application cannot be preserved reliably.**

### Phase 1 — centralize pill display arbitration

- Replace overlapping review/notice/quick-control ownership with one derived
  display mode or reducer.
- Encode the priority and transition table from this plan.
- Preserve existing dictionary review, copy rescue, notices, listening,
  transcribing, inserted, error, and hands-free behavior.
- Add transition tests before visual work expands the surface.

### Phase 2 — Quick Controls window mode

- Add a dedicated Quick Controls OS-window geometry and bottom margin.
- Add `set_pill_quick_controls_mode` or generalize the current review/notice
  resizing API into a typed window mode.
- Avoid a collection of conflicting boolean resize commands.
- Ensure all error paths restore compact geometry and click-through behavior.

Suggested Rust model:

```rust
enum PillWindowMode {
    Compact,
    Review,
    Notice { interactive: bool },
    QuickControls,
}
```

This enum need not cross serialization if a narrower command is cleaner, but
the backend should still have one authoritative geometry resolver.

### Phase 3 — controls and synchronization

- Render profile selector.
- Render Start hands-free.
- Render Open VoiceWave.
- Use narrow backend mutations.
- Synchronize changes back to the main window.
- Show brief inline success/error state without producing another pill notice.

### Phase 4 — accessibility and motion

- Restore appropriate focus-visible treatment for interactive modes.
- Add keyboard/radiogroup behavior.
- Add reduced-motion behavior.
- Verify contrast in Light and Dark themes.
- Test long labels at 125–200% scaling.

### Phase 5 — desktop smoke and restraint audit

- Run the complete manual matrix below.
- Confirm no new automatic prompt was introduced.
- Confirm every setting still has one authoritative persisted value.
- Record screenshots/video for review before considering the plan DONE.

---

## State transition table

| Current | Event | Next | Notes |
|---|---|---|---|
| Compact | User opens controls | Quick Controls | Explicit user intent |
| Quick Controls | Outside click / Escape (focused path) / idle timeout | Compact | Restore click-through |
| Quick Controls | Select profile | Quick Controls | Inline confirmation |
| Review | User opens controls | Quick Controls | Review item returns to queue |
| Quick Controls | Start hands-free | Recording | Collapse immediately |
| Quick Controls | Open app | Compact | Main window may focus |
| Any idle mode | Dictation accepted | Recording | Recording wins |
| Recording | Key release/pause | Transcribing | Existing behavior |
| Recording | Escape | Compact/error | Existing cancel contract |
| Transcribing | Insertion succeeds | Inserted | Existing animation |
| Transcribing | Insertion fails | Recovery | Preserve transcript |
| Notice | Recording starts | Recording | Queue/drop notice |
| Review | Recording starts | Recording | Preserve review queue |
| Quick Controls | Blocking recovery | Recovery | Recovery wins |

---

## Testing strategy

### Rust/unit tests

- Geometry resolver maps every `PillWindowMode` correctly.
- Quick Controls entry/exit restores pointer behavior even after an error.
- The narrow profile command mutates only its intended setting.
- Settings normalization remains intact.
- Hands-free start command respects current controller state.
- Display priority function is deterministic if implemented in Rust.

### Frontend/component tests

- Idle explicit-open event renders Quick Controls.
- Selected profile reflects the backend snapshot.
- Profile selection calls the correct bridge with the exact typed value.
- Start hands-free collapses and invokes once.
- Open VoiceWave invokes once.
- Escape closes Quick Controls when idle.
- Recording state supersedes Quick Controls.
- Recovery supersedes Quick Controls.
- Existing copy-rescue and add-dictionary actions remain functional.
- Failed setting mutation leaves the panel usable and shows local error copy.
- Focusable elements have accessible roles/names.

### Required Windows desktop smoke matrix

1. Open the pill over Notepad, choose Writing, dictate, and confirm insertion
   returns to Notepad.
2. Repeat over Cursor/VS Code and a terminal.
3. Click outside the visible compact pill window and confirm the underlying app
   receives the click.
4. Open/collapse repeatedly at 100%, 125%, 150%, and 200% display scaling.
5. Test primary and secondary monitors with different scaling.
6. Test taskbar bottom, left/right if supported, and auto-hide taskbar.
7. Test maximized and full-screen applications, including a
   fullscreen-exclusive game — the pill must be click-through or hidden there.
8. Open Quick Controls, start hands-free, finish by pause, and verify insertion.
9. Open Quick Controls and trigger normal hold-to-talk; recording must win.
10. Trigger an insertion failure and confirm rescue text remains copyable.
11. Trigger dictionary review and confirm it never overlaps Quick Controls,
    and that an interrupted review item re-emits after the card closes.
12. Tap the pill on a touch-capable device and confirm open/activation
    behavior matches the mouse path.
13. Change profile in the main app while the pill is open and verify the pill
    reconciles without overwriting it.
14. Exercise keyboard-only operation (shortcut/tray entry) and reduced-motion
    mode; verify the previously focused app is restored on collapse.
15. Restart VoiceWave and confirm the selected profile persisted.

Automated tests cannot replace checks 1–8 because Windows activation,
always-on-top hit-testing, and insertion focus are the main feature risks.

---

## Success criteria

V1 is successful when:

- The pill can be intentionally opened without stealing the insertion target.
- Transparent window area does not intercept unrelated clicks.
- Profile switching takes no more than two interactions after opening.
- Hands-free can be started from the card in one interaction.
- Keyboard-only users can open, operate, and close the card via the
  shortcut/tray entry, with the previously focused app restored.
- Existing dictation and notice states retain priority and behavior.
- Quick Controls never opens automatically.
- Existing automatic notices remain restraint-gated.
- Main-window and pill settings cannot overwrite each other through stale
  snapshots.
- All focused tests, production frontend build, Rust/Tauri compile, and the
  desktop smoke matrix pass.

### Suggested product metrics (privacy-safe, only if analytics opt-in)

- Quick Controls opened (mouse vs shortcut/tray entry).
- Profile changed from Quick Controls.
- Hands-free started from Quick Controls.
- Main app opened from Quick Controls.

Do not record which application was active, transcript content, selected text,
microphone names, or control labels beyond stable event categories.

---

## Failure and rollback behavior

- If Quick Controls fails to load, collapse to the existing compact pill.
- If a profile mutation fails, keep the panel open and show a concise
  retryable error; do not pretend the setting changed.
- If resizing fails, restore compact size and click-through best-effort.
- If focus preservation fails in production validation, disable direct-click
  opening and fall back to the shortcut/tray entry rather than shipping unsafe
  insertion behavior.
- The feature should be isolated behind one user-visible availability setting
  or internal rollout gate until desktop smoke is complete.
- Removing/turning off Quick Controls must leave existing pill status, notices,
  rescue copy, dictionary review, and hands-free indicators unchanged.

---

## Follow-up candidates after V1

Each requires its own product decision and restraint gate:

1. Microphone picker and mic recovery.
2. Undo/copy the last successful insertion.
3. Retry failed transcription.
4. Retry model download.
5. Restart into a downloaded update.
6. Selected-text command entry (must not use the focused entry path — taking
   focus deselects the target text; capture selection first or use the
   no-activate path).
7. User-configured profile memory for an application.
8. Theme switching (cut from V1 — set-once setting, no moment-of-dictation
   value).
9. Temporary mute/pause toggle for VoiceWave (screen share, gaming,
   meetings) — far higher moment-of-use frequency than theme; the strongest
   candidate for the card's next row.

Do not add these opportunistically while implementing V1.

---

## Resolved decisions (2026-08-12)

1. **Clickable pill?** Yes — pursue the Option A spike, and ship the
   shortcut/tray entry alongside it regardless (required for
   keyboard/accessibility and `showFloatingHud`-off).
2. **Selecting a profile keeps the card open?** Yes.
3. **Theme in V1?** No — cut. A set-once setting fails the card's own
   moment-of-dictation inclusion bar, and inside a theme-invariant pill the
   selection has no visible feedback. Moved to follow-ups.
4. **`Start hands-free` primary?** Yes — it is the only control that starts
   something; profiles are modifiers.
5. **Read-only mic display?** No — a non-interactive row invites dead clicks
   and spends the card's tightest real estate on something it cannot act on.
6. **Auto-close on focus loss?** Focused path only (no focus signal exists in
   the mouse path). Mouse path closes on outside click if the mouse hook
   proves cheap, plus dictation-start and the ~15–20 s idle auto-collapse.
7. **Dedicated shortcut even if clicking works?** Yes — it is load-bearing
   (see decision 1), not optional.
8. **Available with pill hidden?** Yes, via tray + shortcut; free once
   decisions 1 and 7 land.

---

## Recommendation

Product boundaries and open decisions are approved (2026-08-12); Phase 0 is
the remaining gate. The feature is promising because the typed-action,
notice, resizing, and animation foundations already exist. The decisive risk
is not visual design; it is whether a bottom-center always-on-top webview can
accept intentional input without stealing focus or blocking clicks intended
for the user's app — and Phase 0 must additionally prove the WebView2
child-HWND hit-testing and the outside-click detection story, the two items
most likely to force the shortcut/tray-only fallback.

If the interaction contract is proven, Pill Quick Controls is a focused,
distinctive VoiceWave feature. If it is not proven, retain the existing calm
status pill and ship the focused shortcut/tray-opened card alone.
