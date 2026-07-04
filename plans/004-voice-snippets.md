# Plan 004: Add voice snippets (trigger phrase → expansion)

> **Executor instructions**: Follow step by step; run every verification and
> confirm the expected result. On any "STOP condition", stop and report. Update
> this plan's row in `plans/README.md` when done.
>
> **Drift check (run first)**: written against commit `d29927d` **plus
> uncommitted working-tree changes** (do NOT `git stash`). Confirm the "Current
> state" excerpts before starting; on mismatch, STOP.

## Status

- **Priority**: P3
- **Effort**: M
- **Risk**: MED (trigger-phrase matching competes with existing finalize heuristics; ordering matters)
- **Depends on**: none — but shares files with plans 001, 002. If executing after them, re-run the drift check; the dictionary store and finalize pipeline may have moved.
- **Category**: direction
- **Planned at**: commit `d29927d` + uncommitted working tree, 2026-07-04

## Why this matters

Snippets — say a short trigger, get a canned expansion ("my address", "sig" →
your email signature) — are an explicitly named v1.1 fast-follow and a Pro
candidate (`Idea.md:513,527`), and a Wispr parity gap. VoiceWave already has the
exact infrastructure a snippet store needs: the personal-dictionary module is an
AES-GCM encrypted JSON store with a manager, Tauri commands, and a settings/UI
plumbing pattern. This plan reuses that shape for snippets and adds a
trigger-match stage in the finalize pipeline. Mostly new UI plus one string
transform; the storage/encryption/plumbing patterns already exist.

## Current state

- `src-tauri/src/dictionary/mod.rs` — the near-template for a snippet store.
  - `DictionaryStore` (`:67-73`): `{ next_id, queue, terms }`, serde camelCase, `#[derive(Default)]`.
  - `EncryptedDictionaryStore` (`:75-81`): `{ version, nonce_b64, ciphertext_b64 }` — the on-disk envelope.
  - `DictionaryManager` (`:83-88`): `{ path, _key_path, key: [u8;32], store }`, constructed via `from_paths(path, key_path)` (`:105`), with AES-GCM encrypt/decrypt and a private `persist(&mut self)`. `new()` (`:91`) resolves `ProjectDirs("com","voicewave","localcore").config_dir().join("dictionary.json")` + `.key`.
  - `add_term`/`get_terms`/`remove_term`/`persist` (`:246-308`) are the CRUD shape to mirror.
- `src-tauri/src/transcript/mod.rs` — where snippet expansion runs.
  - `finalize_pro_transcript` (`:32-60`) pipeline (see Plan 002 for the full excerpt). Snippet expansion should run EARLY — before sentence/list formatting — so an expanded snippet then flows through normal formatting. The natural point is right after `finalize_user_transcript` at `:45` (or before it, on the sanitized text), and it needs the snippet list passed in via `ProTranscriptOptions` (`:23-30`).
  - `stabilize_custom_terms(&text, options.custom_terms)` (`:51`, impl near `:522`) shows the pattern of threading a user-phrase list into finalize — model the snippet-list threading on it.
- `src-tauri/src/state.rs` — `VoiceWaveController` owns managers behind `Arc<Mutex<..>>`; the dictation flow builds `ProTranscriptOptions` from settings + the dictionary terms. A `snippet_manager: Arc<Mutex<SnippetManager>>` follows the `dictionary_manager` pattern exactly (construction in `VoiceWaveController::new`, plus command wrappers).
- `src-tauri/src/lib.rs` — dictionary commands (`:1000-1074`) are the template for snippet CRUD commands; register new commands in `generate_handler![...]`.
- `src/lib/tauri.ts` (dictionary fns `:426-453`), `src/hooks/useVoiceWave.ts`, `src/App.tsx` (dictionary view) — the frontend template for a Snippets settings view.

**Conventions to match:**
- Encrypted-at-rest local store with a sibling `.key`, exactly like `dictionary`/`history` (`DictionaryManager::from_paths`).
- serde camelCase across IPC; `thiserror` error enum; `Result<T, SnippetError>` → `Result<T, String>` at the command via `AppError::Controller`.
- Pure string transform for expansion, composed into `finalize_pro_transcript`.

## Commands you will need

| Purpose | Command | Expected |
|---|---|---|
| Rust | `cd "C:\Users\posan\OneDrive\Desktop\voice vibe\src-tauri" && export PATH="$PATH:/c/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/Common7/IDE/CommonExtensions/Microsoft/CMake/CMake/bin" && cargo test --lib` | `218 passed`+ |
| Frontend | `cd "C:\Users\posan\OneDrive\Desktop\voice vibe" && npx vitest run && npm run build` | 34 pass, built |

## Scope

**In scope:**
- `src-tauri/src/snippet/mod.rs` (NEW — the store/manager, modeled on `dictionary/mod.rs`)
- `src-tauri/src/lib.rs` (register the module; add snippet CRUD commands + list registration)
- `src-tauri/src/state.rs` (own the manager; thread snippet list into `ProTranscriptOptions`; command wrappers)
- `src-tauri/src/transcript/mod.rs` (add `expand_snippets` step + `snippets` field on `ProTranscriptOptions`)
- `src/types/voicewave.ts`, `src/lib/tauri.ts`, `src/hooks/useVoiceWave.ts`, `src/App.tsx` (Snippets settings view)

**Out of scope:**
- Cloud sync of snippets (`cloudSync.ts`) — local only for v1.
- Dynamic/templated snippets (date insertion, cursor placeholders) — plain text expansion only for v1.
- Reordering or changing existing finalize steps beyond inserting the snippet step.

## Git workflow

- Branch: `advisor/004-voice-snippets`
- Commit style: `feat(snippets): local voice snippet expansion`

## Steps

### Step 1: Create the snippet store/manager

Create `src-tauri/src/snippet/mod.rs`, copying the STRUCTURE of `dictionary/mod.rs` (not verbatim — adapt):
```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Snippet {
    pub snippet_id: String,
    pub trigger: String,        // spoken trigger phrase, normalized lowercase
    pub expansion: String,      // literal replacement text
    pub created_at_utc_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct SnippetStore { next_id: u64, snippets: Vec<Snippet> }
```
Add `SnippetError` (thiserror, mirror `DictionaryError`), `SnippetManager` with `new()` → `config_dir().join("snippets.json")` + `.key`, `from_paths`, AES-GCM encrypt/decrypt + `persist` (copy the dictionary crypto helpers), and CRUD: `add_snippet(trigger, expansion)`, `list_snippets()`, `remove_snippet(id)`. Enforce: trigger non-empty, dedupe triggers case-insensitively, cap expansion length (e.g. ≤2000 chars).

Register the module: add `pub mod snippet;` in `lib.rs` (next to `pub mod dictionary;`).

**Verify**: `cargo build --lib` → exit 0.

### Step 2: Unit-test the manager

Add `#[cfg(test)] mod tests` to `snippet/mod.rs` mirroring the dictionary tests (temp-path manager). Cover: add+list round-trip through persist (construct a second manager on the same path, assert the snippet loads), duplicate-trigger dedupe, remove, empty-trigger rejection.

**Verify**: `cargo test --lib snippet::` → all pass.

### Step 3: Snippet expansion in the finalize pipeline

In `transcript/mod.rs`, add:
```rust
/// Replace whole-phrase spoken triggers with their expansion. Longest trigger
/// first so "my work email" wins over "my email". Case-insensitive, word-boundary.
fn expand_snippets(input: &str, snippets: &[(String, String)]) -> String {
    let mut ordered = snippets.to_vec();
    ordered.sort_by_key(|(trigger, _)| std::cmp::Reverse(trigger.split_whitespace().count()));
    let mut text = input.to_string();
    for (trigger, expansion) in &ordered {
        text = replace_boundary_phrase_case_insensitive(&text, trigger, expansion);
    }
    text
}
```
Add `pub snippets: &'a [(String, String)]` to `ProTranscriptOptions`. Call `expand_snippets` in `finalize_pro_transcript` right after `finalize_user_transcript` (`:45`), BEFORE domain corrections/format profile, so expansions flow through normal formatting. Guard: skip if the slice is empty (zero overhead when no snippets).

**Ordering / collision risk (MED):** snippet triggers must run before `apply_domain_corrections` and list/number heuristics so they aren't partially rewritten first. Longest-match-first prevents shorter triggers shadowing longer ones. Document this ordering in a comment.

**Verify**: `cargo build --lib` → exit 0.

### Step 4: Thread snippets from state → finalize

In `state.rs`: own `snippet_manager` in `VoiceWaveController` (construct in `new()` like `dictionary_manager`; wrap in `Arc<Mutex<..>>`). In the dictation flow where `ProTranscriptOptions` is built, load the snippet list (lock manager, map to `Vec<(trigger, expansion)>`) and pass it. Add controller wrappers `add_snippet`/`list_snippets`/`remove_snippet` + a `voicewave://snippet` event (or reuse a generic refresh) mirroring the dictionary wrappers.

**Verify**: `cargo test --lib` → `218 passed`+ (no regressions).

### Step 5: Tauri commands + registration

In `lib.rs`, add `#[tauri::command]`s `add_snippet(app, runtime, trigger, expansion)`, `list_snippets(runtime)`, `remove_snippet(app, runtime, snippet_id)` modeled on the dictionary commands, and add them to `generate_handler![...]`. Import `Snippet` where `DictionaryTerm` is imported.

**Verify**: `cargo test --lib` → pass.

### Step 6: Frontend Snippets view

- `src/types/voicewave.ts`: add `Snippet` interface (camelCase).
- `src/lib/tauri.ts`: add `addSnippet`, `listSnippets`, `removeSnippet` bridge fns (model after dictionary fns).
- `src/hooks/useVoiceWave.ts`: hold snippet state + expose actions; load on mount like dictionary terms.
- `src/App.tsx`: add a "Snippets" section (in Settings or as a sidebar view — match how Dictionary is surfaced) with a list of trigger→expansion rows, an add form (two inputs), and a remove button per row. Reuse dictionary-view styling.

**Verify**: `npx vitest run` (34+), `npm run build` (clean).

## Test plan

- Rust: manager tests (Step 2) + transcript tests for `expand_snippets`: single trigger expands; longest-match-first ("my work email" beats "my email"); no snippets = identity; trigger inside a larger word is not expanded (boundary).
- Frontend: suite stays green; add a `tauri.test.ts` assertion for the new bridge commands if that pattern exists.
- Verification: `cargo test --lib` (≥226 with new tests), `npx vitest run` (34+), `npm run build`.

## Done criteria

- [ ] `cargo test --lib` exits 0 with new snippet + transcript tests passing
- [ ] A snippet added via the UI expands when its trigger is dictated (in the Pro finalize path)
- [ ] Longest-trigger-first resolution verified by test
- [ ] Empty snippet list adds zero overhead (early return) and no behavior change
- [ ] `npx vitest run` and `npm run build` pass
- [ ] No out-of-scope files modified
- [ ] `plans/README.md` updated

## STOP conditions

- The dictionary module structure doesn't match the excerpts (drift).
- Threading `snippets` into `ProTranscriptOptions` collides with Plan 002's `spoken_edit_commands` field addition (if 002 landed first, both fields coexist — just add yours; if there's a real conflict, report).
- `expand_snippets` ordering causes an existing transcript test to fail in a way that reveals a real heuristic collision — report with the failing case rather than reordering existing steps.
- Verification fails twice after a reasonable fix.

## Maintenance notes

- Snippets are Pro-flagged in the roadmap; this plan makes them universal in the finalize path. If Pro-gating is wanted, gate the `expand_snippets` call on the entitlement like other Pro features.
- Reviewer: scrutinize expansion ordering vs domain corrections / numbered-list detection — the collision risk is where bugs hide.
- Deferred: templated snippets (date/cursor), snippet import/export (would reuse Plan 001's pattern), cloud sync.
