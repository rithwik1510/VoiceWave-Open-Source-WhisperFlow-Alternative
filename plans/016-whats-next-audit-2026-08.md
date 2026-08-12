# 016 — What's Next: Full-Repo Audit & Priority Order (2026-08)

Captured 2026-08-12 after a full sweep of the codebase, plans 001–015, the
roadmap, and the working tree. Status: **decision doc / brainstorm** (like 009)
— pick items into numbered plans as they're scheduled.

---

## The headline finding

**VoiceWave has significantly more product built than shipped.** The
engineering is ahead of the product surface: conservative gates, crash-safe
stores, signed manifests — but also dark env flags, no-op commands, and dead
settings. In that situation, *exposure work beats new-feature work on ROI*:
several competitor-headline features are one wiring session away.

### Built but not shipped / not reachable

1. **Hands-free mode is half-landed in the working tree** (~1,000 uncommitted
   lines on `dark-mode-v1.2`): hold-to-talk vs hands-free `DictationControlMode`,
   double-tap detection, Escape-to-cancel, end-on-silence auto-stop — with
   tests. This is brainstorm 009 item #2 (the accessibility/RSI wedge; Dragon's
   orphaned market). No plan file; tangled onto the dark-mode branch — needs
   untangling into its own commits/plan.
2. **Dark mode (plan 014) is done but unmerged**; desktop smoke test pending.
3. **Streaming live preview is built and switched off.** The incremental
   pre-release decode path (`run_incremental_preview_decode`, transcript
   merging, `holdToFirstDraftMs` telemetry) exists and is tested, gated behind
   `VOICEWAVE_INCREMENTAL_PRE_RELEASE_DECODE_ENABLED` (default false,
   `src-tauri/src/state.rs:1144`). The plans/README "rejected: architecture
   trap" entry predates this code existing — that is the new evidence the
   roadmap requires to re-litigate. 009 calls streaming the single biggest
   felt-quality gap vs Wispr and Win+H.
4. **Per-app profiles are ~90% built and mis-plumbed.**
   `active_profile_behavior` (`src-tauri/src/state.rs:3431`) reads
   `app_profile_overrides.active_target` — a static setting with no UI —
   while `foreground_process_exe_name()` already detects the real focused app
   for terminal handling. Routing the detected app into profile selection +
   the polish prompt is mostly wiring. This is Wispr Flow's marquee paid
   feature ("casual in Slack, formal in email").
5. **Shipped features that no user can reach (dead settings, no UI):**
   - `pillActionSuggestions` — gates the plan-003 pill actions, defaults
     false, no toggle → a shipped feature is dark for every user.
   - `showFloatingHud` — users cannot hide the pill.
   - Code-mode casing/fenced-block options — the Pro Tools help text points
     to a settings UI that does not exist (`src/App.tsx:2625`).
   - Hotkey rebinding — `update_hotkey_config` discards its argument and
     re-stamps the locked constants (`src-tauri/src/state.rs:1975`); every
     competitor allows rebinding.
   - Also settable-but-unrendered: `decodeMode`, `proPostProcessingEnabled`,
     `preferClipboardOnlyForTerminals`, cue-sound toggle
     (`VOICEWAVE_CUE_SOUNDS` env-only).

---

## Recommended order

### 1. Land what's in flight
Finish the hands-free WIP (write a small plan file, separate it from the
dark-mode commits), merge dark mode after its smoke test. Two real features,
near-zero new engineering.

### 2. Fix distribution: winget + README truth-sync
`winget install VoiceWave.LocalCore` is the README's primary install path and
has been broken since v0.5.0 (PAT); the published listing is six releases
stale; the fix exists locally but was never pushed. Both ROADMAP.md and 009
rank this above every feature on ROI. While there, do a README truth-sync —
it currently promises CUDA auto-detection and cloud-synced dictionaries that
no shipped build delivers.

### 3. Finish the wiring: per-app profiles, then streaming preview
- Per-app profiles: route the detected foreground app into
  `active_profile_behavior` + the polish prompt; small settings UI
  (per-app tone + polish on/off).
- Streaming preview: flip the flag behind a settings toggle; validate partial
  quality and CPU cost on CPU-only machines first — if partials look bad
  there, that's the honest reason to keep it off.

These two convert "free offline alternative" into "feels better than the paid
cloud thing."

### 4. Multilingual — the cheapest genuinely-new feature
`language` is hardcoded `"en"` in four places
(`faster_whisper.rs:191,1095`, `worker.py:398`, `inference/mod.rs:1385`) and
`set_translate(false)` in one. large-v3-turbo is already multilingual-capable
— the English-only ceiling is entirely self-imposed. A language setting opens
~98 languages for close to zero engineering; translation is one flag more.

### 5. Paper-cut batch (one focused session)
- History export ignores active search/tag/star filters
  (`state.rs:3231` hardcodes `SessionHistoryQuery::default()`) and dead-ends
  in a tiny `<pre>` — no save-to-file, no copy button.
- No per-record history delete (only Clear All / Prune Now).
- Tag button silently no-ops when the tag input is empty; no tag removal.
- Polish model download failure is terminal — no retry button.
- One global error string, ~35 call sites, rendered as a bottom red banner
  with no recovery actions.
- Dictionary export uses a browser blob download with a success toast that
  fires regardless of outcome.
- `classify_insertion_target` hardcodes ~10 apps; Word/Outlook/Teams/Discord/
  Obsidian/JetBrains all bucket as "desktop", degrading Stats top-apps.
- Onboarding has no model-download failure branch or mic-permission step.

### 6. Next big swing: command mode on selected text
"Make this concise," "fix the grammar," "translate this" on highlighted text.
Every ingredient exists: the spoken-command parser, the on-device LLM, the
clipboard/SendInput machinery (capture selection via Ctrl+C, rewrite, paste
back). Wispr gates this behind $15/mo Pro; shipping it free and offline is
the loudest possible message.

### 7. Meeting Mode — follow plan 015's own gate, don't jump in
Plan 015 is a decision draft (uncommitted, zero implementation) and its
recommendation is right: meeting capture is a second product pillar with much
higher reliability stakes than dictation. Path:
**quick notes → audio-file import experiment → WASAPI capture spike →
explicit V1 go/no-go.** Quick capture / voice notes stands alone as a
mid-size feature even if Meeting Mode is ultimately rejected.

---

## Backlog worth remembering (unordered)

- Self-correcting dictionary: close the loop from "user immediately corrected
  the pasted word" → promote to dictionary (009 item #8).
- GPU acceleration pack: workers already auto-detect CUDA; only delivery of
  the nvidia wheels is missing (ROADMAP item 6).
- Launch-at-login setting (no autostart plugin today).
- Stats export / shareable card ("screenshots are free marketing").
- Custom polish prompts (SuperWhisper's whole model is user-authored modes).
- Snippet variables (`{{date}}`, cursor placeholder) — deliberate tension
  with the literal-expansion protection guarantee; decide explicitly.
- Spoken punctuation outside code mode ("period", "question mark",
  "scratch that").
- AVX2 requirement is undeclared — no capability probe, worker just dies on
  pre-2013 CPUs (`build-embedded-runtime.ps1:139`).
- Async retry-with-correction pass (plan 010 Phase 6, still open;
  `history/mod.rs:78`).
- Dead code cleanup: unused `src/components/Sidebar.tsx` / `StatePill.tsx`,
  nine `@deprecated` legacy-profile blocks in `App.tsx`.
- Fixed 200-record history cap makes "forever" retention not actually forever.
- Parakeet as a second engine (Tier-3 strategic bet; engine-agnostic
  inference layer first).

---

## Why this ordering

Each item in steps 1–3 ships a competitor-headline feature for wiring-level
effort, and step 2 fixes the funnel those features flow through. The genuinely
new bets (command mode, meetings) come after, when each release in between has
already given users something visible. New-feature work has worse ROI than
exposure work while built-but-dark inventory remains.
