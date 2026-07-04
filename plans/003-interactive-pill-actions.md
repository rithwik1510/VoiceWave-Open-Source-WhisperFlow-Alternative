# Plan 003: Generalize the pill-notice channel into typed, one-tap interactive actions

> **Executor instructions**: Follow step by step; run every verification and
> confirm the expected result before proceeding. On any "STOP condition", stop
> and report. When done, update this plan's row in `plans/README.md`.
>
> **Drift check (run first)**: written against commit `d29927d` **plus
> uncommitted working-tree changes** — the pill-notice system itself is
> uncommitted working-tree code, so do NOT `git stash`. Open each "Current
> state" file and confirm the excerpts before starting; on mismatch, STOP.

## Status

- **Priority**: P2
- **Effort**: M (the typed-action framework) — then each additional action is cheap
- **Risk**: MED (over-prompting / focus behavior — mitigations in plan)
- **Depends on**: none (builds on the already-shipped pill-notice system)
- **Category**: direction
- **Planned at**: commit `d29927d` + uncommitted working tree, 2026-07-04

## Why this matters

VoiceWave just shipped a Dynamic-Island pill-notice system. Its payload already
carries a single hard-coded `action: "copyTranscript"` that makes the pill
interactive and renders one button. Wispr-class UX is low-friction in-context
recovery: a mic-muted notice with a "Select microphone" button, a low-volume
notice with "Restore volume", a post-dictation "Add '<term>' to dictionary?"
one-tap. The plumbing (interactive pill window, one working action, the
`action` field) already exists; this generalizes `action` from a single string
into a small set of typed actions with frontend handlers and backend routing,
so new one-tap actions become incremental. The correction candidates that would
feed an "add term" action are already computed on every dictation.

## Current state

- `src-tauri/src/state.rs` — the notice payload and emit helpers (working-tree code):
  - `PillNoticePayload` (`state.rs:~104-124`):
    ```rust
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct PillNoticePayload {
        pub id: u64,
        pub severity: String,      // "info" | "warning" | "error"
        pub title: String,
        pub detail: Option<String>,
        pub duration_ms: u64,
        pub transcript: Option<String>,   // rescue payload
        pub action: Option<String>,       // "copyTranscript" today
    }
    ```
  - `emit_pill_notice(app, severity, title, detail, duration_ms)` and
    `emit_pill_rescue(app, severity, title, detail, transcript, action)` are the
    two emit helpers. Many notice sites call `emit_pill_notice` with no action
    (mic guard, CPU fallback, cold engine, poor audio, no-speech). The mic
    low-volume auto-restore path already performs an inline restore in Rust
    (search `emit_pill_notice` and the mic-guard block in `state.rs`).
  - `derive_correction_candidates(previous, current) -> Vec<String>` (`state.rs:510`)
    and the per-dictation `correction_candidates` are already computed; the
    dictionary "review" popup consumes them. These are the natural source for an
    "Add term" action.
- `src-tauri/src/lib.rs` — pill commands (working-tree code):
  - `set_pill_notice_mode(app, notice_mode: bool, interactive: Option<bool>)` sizes/positions the pill window and toggles click-through.
  - `copy_text_to_clipboard(text: String)` — the command behind the current copy action.
  - Commands are registered in `tauri::generate_handler![...]` (`lib.rs`); new commands must be added there.
- `src/types/voicewave.ts` — `PillNoticePayload` TS mirror:
  ```ts
  export interface PillNoticePayload {
    id: number;
    severity: "info" | "warning" | "error";
    title: string;
    detail: string | null;
    durationMs: number;
    transcript: string | null;
    action: "copyTranscript" | null;
  }
  ```
- `src/components/FloatingPill.tsx` — renders notices and handles the copy action:
  - `noticeInteractive = noticeActive && notice?.action != null` (`FloatingPill.tsx:223`) drives window interactivity.
  - `handleCopyTranscript` (`:237-252`) calls `copyTextToClipboard(notice.transcript)`, flips a `copied` state, and re-arms dismissal; hover pause/resume at `:257-263`.
  - The notice panel JSX renders the title/detail/dot and, when `action === "copyTranscript"`, a Copy button (search for `vw-pill-action-copy` in `FloatingPill.tsx` and `src/pill.css`).
- `src/lib/tauri.ts` — bridge fns `setPillNoticeMode`, `copyTextToClipboard`, `listenVoicewavePillNotice`.

**Conventions to match:**
- Notices ride the single `voicewave://pill-notice` event; do NOT invent new channels.
- The pill window is normally click-through; only actionable notices enable pointer input (`set_pill_notice_mode(.., interactive=true)`).
- Rust emit helpers construct the payload; the frontend reacts. Keep that split.
- Restraint is a stated product value — do NOT surface an action on every dictation.

## Commands you will need

| Purpose | Command | Expected |
|---|---|---|
| Rust | `cd "C:\Users\posan\OneDrive\Desktop\voice vibe\src-tauri" && export PATH="$PATH:/c/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/Common7/IDE/CommonExtensions/Microsoft/CMake/CMake/bin" && cargo test --lib` | `218 passed`+ |
| Frontend | `cd "C:\Users\posan\OneDrive\Desktop\voice vibe" && npx vitest run && npm run build` | 34 pass, built |

## Scope

**In scope:**
- `src-tauri/src/state.rs` (extend the payload/action model; emit an "add term" actionable notice; wire action routing where the action executes in Rust)
- `src-tauri/src/lib.rs` (add command(s) for the new action(s); register them)
- `src/types/voicewave.ts` (widen the `action` type)
- `src/components/FloatingPill.tsx` (render buttons per action type; dispatch to the right handler)
- `src/pill.css` (reuse `.vw-pill-action` styles; add a variant only if needed)
- `src/lib/tauri.ts` (bridge fn(s) for the new action command(s))

**Out of scope:**
- Adding many actions at once. This plan ships the FRAMEWORK plus exactly ONE new action ("Add '<term>' to dictionary?") as the proof. Additional actions (Select microphone, Retry on GPU) are follow-ups once the framework is in.
- Changing the rescue-transcript copy flow behavior (keep it working exactly as-is).
- The dictionary "review" popup (separate pill mode) — leave it.

## Git workflow

- Branch: `advisor/003-interactive-pill-actions`
- Commit style: `feat(pill): typed interactive notice actions`

## Steps

### Step 1: Model a typed action with an optional payload

Rather than a bare `action: Option<String>`, introduce a small structured action so the frontend knows what button to show and what data it carries. In `state.rs`, add:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PillAction {
    pub kind: String,          // "copyTranscript" | "addDictionaryTerm"
    pub label: String,         // button text, e.g. "Add \"foo\""
    pub value: Option<String>, // action data, e.g. the term to add
}
```
Change `PillNoticePayload.action` from `Option<String>` to `Option<PillAction>`. Update `emit_pill_rescue` so the existing copy action constructs `PillAction { kind: "copyTranscript", label: "Copy".into(), value: None }` — preserving today's behavior. (The rescue transcript still travels in `payload.transcript`; `copyTranscript` reads that, so `value` stays `None` for it.)

**Verify**: `cargo build --lib` → exit 0.

### Step 2: Add a Rust command for the new action

Add `#[tauri::command] add_dictionary_term_from_pill(app, runtime, term: String)` in `lib.rs` that calls the existing controller `add_dictionary_term` (or reuse the existing `add_dictionary_term` command directly from the frontend — prefer reuse; only add a new command if the existing one's signature doesn't fit). Confirm `add_dictionary_term` already exists (`lib.rs:1062`) — it does, so the frontend can call the existing `addDictionaryTerm` bridge for this action and NO new Rust command is needed. State that you reused it.

**Verify**: n/a (no new Rust command if reusing) — proceed.

### Step 3: Emit an actionable "Add term?" notice after dictation

In the dictation flow in `state.rs`, after `correction_candidates` are derived and insertion succeeded, when there is exactly one high-confidence candidate AND the restraint gate allows it (see below), emit an actionable notice:
```rust
emit_pill_action_notice(
    &app, "info",
    "Add to dictionary?",
    None,
    PillAction { kind: "addDictionaryTerm".into(),
                 label: format!("Add \"{term}\""),
                 value: Some(term.clone()) },
    6_000,
);
```
Add a small `emit_pill_action_notice` helper next to the existing emit helpers (mirror `emit_pill_rescue` but take a `PillAction` and no transcript).

**Restraint gate (MED-risk mitigation):** the existing dictionary review popup already surfaces suggestions; do NOT double-surface. Reuse a cooldown like the existing `take_notice_cooldown` pattern (search `take_notice_cooldown` in `state.rs`) so this fires at most once per N minutes, and only when the dictionary "review mode" is NOT already showing that term. If unsure whether the review popup already covers this, gate this behind an off-by-default setting `pill_action_suggestions: bool` (add to `VoiceWaveSettings`) so it can't regress the current calm UX. Prefer the setting-gated approach and default it OFF; state your choice.

**Verify**: `cargo test --lib` → `218 passed`+ (no regressions).

### Step 4: Frontend — widen the type and render per-action buttons

- `src/types/voicewave.ts`: replace `action: "copyTranscript" | null` with:
  ```ts
  export interface PillAction {
    kind: "copyTranscript" | "addDictionaryTerm";
    label: string;
    value: string | null;
  }
  // and: action: PillAction | null;
  ```
- `FloatingPill.tsx`:
  - `noticeInteractive` becomes `noticeActive && notice?.action != null` (already the case; the shape just changed from string to object).
  - Replace the copy-specific button render with a switch on `notice.action.kind`:
    - `"copyTranscript"` → existing Copy button + `handleCopyTranscript`.
    - `"addDictionaryTerm"` → a button labeled `notice.action.label` whose handler calls `addDictionaryTerm(notice.action.value!)` (import from `../lib/tauri`), then flips to a "Added ✓" confirmation and dismisses after ~1200ms (reuse the `copied`/`scheduleNoticeDismiss` pattern; generalize `copied` to a small `actionDone` state if cleaner).
  - Keep hover-pause and the fresh-id reset working for both.
- `src/pill.css`: reuse `.vw-pill-action` / `.vw-pill-action-copy`. Add a `.vw-pill-action-add` only if a distinct style is wanted; otherwise reuse.

**Verify**: `npx vitest run` (34+) and `npm run build` (clean).

### Step 5: Keep the copy rescue flow identical

Manually re-trace: a failed insertion still emits a rescue notice with `action.kind === "copyTranscript"`, the Copy button still copies `notice.transcript`, flips to "Copied ✓", and collapses. No behavior change for that path.

**Verify**: `npx vitest run` still 34+ (the rescue/notice tests, if any, still pass); `npm run build`.

## Test plan

- Rust: add a unit test for `PillAction` serde (camelCase, `kind`/`label`/`value`) and, if you added the restraint setting, a test that the emit is gated. Model after existing `state.rs` payload serde tests (grep `serde_json::to_string` in `state.rs` tests).
- Frontend: if `FloatingPill` has tests, add one asserting an `addDictionaryTerm` action renders the labeled button and calls the bridge; otherwise rely on the suite staying green and manual trace in Step 5.
- Verification: `cargo test --lib` (219+), `npx vitest run` (34+), `npm run build`.

## Done criteria

- [ ] `cargo test --lib` exits 0, no regressions (≥218; +1 if serde test added)
- [ ] The existing copy-on-failed-insertion rescue works unchanged
- [ ] A new `addDictionaryTerm` action renders a labeled button that adds the term and confirms
- [ ] New actionable notice is restraint-gated (setting-off-by-default or cooldown) — cannot spam
- [ ] `npx vitest run` and `npm run build` pass
- [ ] No out-of-scope files modified
- [ ] `plans/README.md` updated

## STOP conditions

- `PillNoticePayload` / `FloatingPill.tsx` action handling doesn't match the excerpts (drift — the pill system is uncommitted, so confirm it's present first).
- Changing `action` from string to object ripples into more call sites than the emit helpers + FloatingPill (grep `\.action` usages first; if a test or other consumer breaks unexpectedly, report).
- Verification fails twice after a reasonable fix.

## Maintenance notes

- Adding future actions ("selectMicrophone", "retryOnGpu") is now: a new `PillAction.kind`, a Rust emit site, and a `FloatingPill` switch arm + handler. Keep the restraint discipline — every new actionable notice needs a cooldown or a clear one-shot trigger.
- Reviewer should scrutinize the restraint gate (Step 3) hardest — the risk here is UX regression from over-prompting, not correctness.
- Interactive notices make the pill briefly clickable; confirm this never steals focus from the user's target app (the app's "no focus theft" value). The pill window is `always_on_top` + `skip_taskbar`; verify `set_pill_notice_mode(.., interactive=true)` doesn't call `set_focus`.
