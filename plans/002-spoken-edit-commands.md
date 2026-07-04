# Plan 002: Promote spoken edit commands to always-on (new line / new paragraph / bullet)

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving on. If a
> "STOP condition" occurs, stop and report — do not improvise. When done,
> update this plan's status row in `plans/README.md`.
>
> **Drift check (run first)**: written against commit `d29927d` **plus
> uncommitted working-tree changes** (do NOT `git stash`). Open each file in
> "Current state" and confirm the excerpts match before starting; on mismatch,
> STOP.

## Status

- **Priority**: P2
- **Effort**: S (the two structural commands) — the "undo last sentence" command is explicitly deferred to a follow-up (see Maintenance notes)
- **Risk**: MED (spoken commands can collide with literal dictation — mitigation is the core of this plan)
- **Depends on**: none
- **Category**: direction
- **Planned at**: commit `d29927d` + uncommitted working tree, 2026-07-04

## Why this matters

Wispr-class dictation lets users speak structure ("new line", "new paragraph",
"bullet point") without reaching for the keyboard. VoiceWave **already has** the
"new paragraph" → `\n\n` and "next line" → `\n` mappings — but they are trapped
inside two specific format profiles (Academic/Concise), so a default-profile
user speaking "new line" gets the literal words instead of a line break. This
plan promotes those structural commands to always-on (independent of format
profile) with disambiguation so literal usage still works, and adds a spoken
"bullet"/"bullet point" list command. It's mostly wiring existing primitives
into the always-on path — the roadmap names these explicitly (`Idea.md:515`).

## Current state

- `src-tauri/src/transcript/mod.rs` — the post-processing pipeline.
  - The main entry `finalize_pro_transcript` (`transcript/mod.rs:32-60`) runs the
    Pro pipeline. The relevant tail:
    ```rust
    let mut text = finalize_user_transcript(&cleaned);        // :45
    if text.is_empty() { return text; }
    text = apply_domain_corrections(&text, options.domain_packs);   // :50
    text = stabilize_custom_terms(&text, options.custom_terms);     // :51
    text = apply_format_profile(&text, options.format_profile);     // :52
    text = apply_app_profile_behavior(&text, options.app_profile_behavior); // :53
    if options.code_mode.enabled { text = apply_code_mode(&text, options.code_mode); } // :55-57
    normalize_output_whitespace(&text)   // :59
    ```
  - The existing structural mappings live INSIDE profile functions only.
    `apply_writing_profile` (`transcript/mod.rs:555-568`):
    ```rust
    fn apply_writing_profile(input: &str) -> String {
        let mut text = input.to_string();
        text = replace_boundary_phrase_case_insensitive(&text, "don't", "do not");
        // ...
        text = replace_boundary_phrase_case_insensitive(&text, "new paragraph", "\n\n");
        text = replace_boundary_phrase_case_insensitive(&text, "next line", "\n");
        if let Some(list) = format_spoken_numbered_list(&text) { return list; }
        text
    }
    ```
    `apply_study_profile` (`:570-587`) repeats the same two mappings. `apply_format_profile` (`:534-553`) dispatches to these per `FormatProfile`; `FormatProfile::Default` returns the input unchanged (`:536`), which is why default-profile users get no structural commands.
  - `replace_boundary_phrase_case_insensitive(text, from, to)` is the existing helper that replaces a phrase only at word boundaries (use it — do not hand-roll matching).
  - `apply_app_profile_behavior` (`:667-698`) already builds bullet lists from `;`-separated segments when `behavior.auto_list_formatting` is set and there are ≥3 segments — but that's driven by app-profile settings, not a spoken command.
  - **Free-tier path**: `finalize_user_transcript` (`:10-21`) is the non-Pro finalizer (sanitize → numbered-list → sentence fragment). Decide whether structural commands are Pro-only or universal — see Step 1.
- `src-tauri/src/state.rs` — calls `finalize_pro_transcript` in the dictation flow (grep `finalize_pro_transcript(` in `state.rs`; the call passes `ProTranscriptOptions` built from settings). Free-tier dictations use `finalize_user_transcript` / `sanitize_user_transcript` instead.
- `src-tauri/src/settings/mod.rs` — `VoiceWaveSettings` struct holds feature toggles (e.g. `code_mode`, `format_profile`). A new `spoken_edit_commands: bool` toggle would live here (default `true`).
- `src-tauri/src/transcript/mod.rs` tests: there is a `#[cfg(test)] mod tests` at the bottom with many string-in/string-out cases — model new tests after those (they call the pipeline functions directly with literal inputs and assert exact output strings).

**Conventions to match:**
- Pure string→string transform functions, composed in `finalize_pro_transcript`. No I/O, no state.
- Use `replace_boundary_phrase_case_insensitive` for phrase replacement.
- Settings fields are serde camelCase with a `#[serde(default)]` where backward compat matters; see how `pro_post_processing_enabled` / `prefer_clipboard_only_for_terminals` are defined and defaulted in `settings/mod.rs`.

## Commands you will need

| Purpose | Command | Expected |
|---|---|---|
| Rust tests | `cd "C:\Users\posan\OneDrive\Desktop\voice vibe\src-tauri" && export PATH="$PATH:/c/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/Common7/IDE/CommonExtensions/Microsoft/CMake/CMake/bin" && cargo test --lib transcript::` | new + existing transcript tests pass |
| Full Rust | same prefix + `cargo test --lib` | `218 passed` or more |
| Frontend | `cd "C:\Users\posan\OneDrive\Desktop\voice vibe" && npx vitest run && npm run build` | 34 pass, built |

## Scope

**In scope:**
- `src-tauri/src/transcript/mod.rs` (add an always-on `apply_structural_commands` step + tests)
- `src-tauri/src/settings/mod.rs` (add the `spoken_edit_commands` toggle, default true)
- `src-tauri/src/state.rs` (thread the toggle into `ProTranscriptOptions` / the free-tier path)
- `src/types/voicewave.ts` + `src/hooks/useVoiceWave.ts` + `src/App.tsx` (surface the toggle in Settings)

**Out of scope:**
- "Undo last sentence" — needs an insertion-retract that coordinates with `correction_session`; deferred to a separate plan (Maintenance notes).
- Reworking the existing per-profile mappings' other behaviors (contraction expansion, study-note sectioning) — leave them; just avoid double-applying the structural mappings (Step 1 handles dedupe).
- Code-mode symbol mapping (`apply_code_mode`) — untouched.

## Git workflow

- Branch: `advisor/002-spoken-edit-commands`
- Commit style: `feat(transcript): always-on spoken edit commands`

## Steps

### Step 1: Add an always-on `apply_structural_commands` transform

In `transcript/mod.rs`, add:
```rust
/// Spoken structural commands that apply regardless of format profile.
/// Recognized only as standalone boundary phrases so literal usage
/// ("start a new paragraph in your essay") is not rewritten.
fn apply_structural_commands(input: &str) -> String {
    let mut text = input.to_string();
    text = replace_boundary_phrase_case_insensitive(&text, "new paragraph", "\n\n");
    text = replace_boundary_phrase_case_insensitive(&text, "new line", "\n");
    text = replace_boundary_phrase_case_insensitive(&text, "next line", "\n");
    // "bullet point" / "bullet" starts a new bulleted line.
    text = replace_boundary_phrase_case_insensitive(&text, "bullet point", "\n- ");
    text = replace_boundary_phrase_case_insensitive(&text, "new bullet", "\n- ");
    text
}
```
Call it in `finalize_pro_transcript` as a new step, gated by the settings toggle. Because `apply_writing_profile`/`apply_study_profile` ALSO map "new paragraph"/"next line", **remove those two lines from both profile functions** (they become redundant once the always-on step runs) so the mapping isn't applied twice — verify by re-reading `:560-561` and `:576-577` and deleting exactly those `replace_boundary_phrase_case_insensitive(... "new paragraph"/"next line" ...)` lines. Keep everything else in those functions.

Add a field to `ProTranscriptOptions` (`transcript/mod.rs:23-30`): `pub spoken_edit_commands: bool`. In `finalize_pro_transcript`, run `apply_structural_commands` right after `finalize_user_transcript` (before `apply_format_profile`) when `options.spoken_edit_commands` is true.

**Disambiguation rule (the MED-risk mitigation):** only replace when the phrase stands as its own token run at a word boundary — `replace_boundary_phrase_case_insensitive` already enforces word boundaries, which is the accepted level of disambiguation here (matches the existing profile behavior). Do NOT add heuristic "did they mean it literally" logic; that's out of scope and the boundary match is the agreed contract.

**Verify**: `cd src-tauri && cargo build --lib` → exit 0.

### Step 2: Free-tier path

Decide scope: to keep behavior consistent, structural commands should also work in the **free** path (`finalize_user_transcript`). Add an optional application there OR document that structural commands are Pro-only. RECOMMENDED: apply `apply_structural_commands` in `finalize_user_transcript` too, gated by the same toggle — but `finalize_user_transcript` currently takes only `&str`. Rather than change its signature (it has other callers — grep `finalize_user_transcript(` first), add the structural step at the `state.rs` call site for the free path, or introduce `finalize_user_transcript_with_commands(input, enabled)` and have `finalize_user_transcript` delegate with `enabled=false` for backward compat. Pick the lower-blast-radius option and state which.

**Verify**: `cargo build --lib` → exit 0; existing callers of `finalize_user_transcript` unchanged.

### Step 3: Settings toggle

- `settings/mod.rs`: add `pub spoken_edit_commands: bool` to `VoiceWaveSettings` with `#[serde(default = "default_true")]` (check whether a `default_true` fn exists; if not, add one or use the existing default pattern used by other bool fields). Set it to `true` in the `Default` impl. Add to any settings normalization if present.
- Thread it into the `ProTranscriptOptions` construction in `state.rs` (grep the `ProTranscriptOptions {` literal in `state.rs`).

**Verify**: `cargo test --lib settings::` and `cargo test --lib` → pass.

### Step 4: Unit tests

Add to `transcript/mod.rs` `mod tests`:
- `structural_commands_apply_in_default_profile`: input `"first line new line second line"` with commands on → contains `"first line\nsecond line"`.
- `new_paragraph_maps_to_double_newline`.
- `bullet_point_starts_new_bulleted_line`: `"buy milk bullet point buy eggs"` → contains `"\n- buy eggs"`.
- `structural_commands_disabled_leaves_text_literal`: same input with the toggle off → the literal words remain.
- `profile_mappings_not_double_applied`: run an Academic-profile transcript containing "new paragraph" and assert exactly one `\n\n` (no `\n\n\n\n` from double application).

**Verify**: `cargo test --lib transcript::` → all pass incl. the 5 new.

### Step 5: Surface the toggle in Settings UI

- `src/types/voicewave.ts`: add `spokenEditCommands: boolean` to `VoiceWaveSettings`.
- `src/hooks/useVoiceWave.ts`: add `spokenEditCommands: true` to `fallbackSettings` and pass-through in `normalizeSettings` (mirror how `proPostProcessingEnabled`/`decodeMode` are defaulted there).
- `src/App.tsx`: add a toggle in the Settings panel near the other post-processing toggles, label "Spoken edit commands", description "Say 'new line', 'new paragraph', or 'bullet point' to add structure." Reuse the existing toggle/setter pattern (model after an existing boolean setting such as `preferClipboardFallback` or `proPostProcessingEnabled`).

**Verify**: `npx vitest run` (34+) and `npm run build` (clean).

## Test plan

- 5 new Rust unit tests in `transcript/mod.rs` (Step 4), modeled after existing string-in/out tests in that module.
- Frontend suite stays green; no new required frontend tests.
- Verification: `cargo test --lib` (223+), `npx vitest run` (34+), `npm run build`.

## Done criteria

- [ ] `cargo test --lib` exits 0, ≥223 passing (5 new)
- [ ] Default-profile dictation with the toggle on converts "new line"/"new paragraph"/"bullet point" to structure
- [ ] Toggle off leaves the literal words
- [ ] No `\n\n\n\n` double-application in Academic/Concise profiles (test asserts)
- [ ] `npx vitest run` and `npm run build` pass
- [ ] No out-of-scope files modified
- [ ] `plans/README.md` updated

## STOP conditions

- The profile functions at `transcript/mod.rs:555-587` don't contain the "new paragraph"/"next line" mappings quoted here (drift).
- `finalize_user_transcript` has more callers than expected and changing the free-tier path would ripple beyond `state.rs` — report and ship Pro-only.
- Any verification fails twice after a reasonable fix.

## Maintenance notes

- **Deferred follow-up — "undo last sentence":** genuinely new; requires tracking the last inserted span and retracting it, coordinating with `CorrectionSession` in `state.rs` and the insertion engine (harder over the clipboard-fallback path than direct injection). Scope as its own plan.
- Reviewer should scrutinize the double-application removal (Step 1) — confirm the profile functions no longer map "new paragraph"/"next line" and the always-on step is the single source.
- If a future locale/i18n effort lands, these English command phrases become locale-dependent.
