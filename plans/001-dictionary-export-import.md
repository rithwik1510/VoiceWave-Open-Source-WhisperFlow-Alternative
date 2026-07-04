# Plan 001: Add export/import to the personal dictionary

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md`.
>
> **Drift check (run first)**: this plan was written against commit `d29927d`
> **plus uncommitted working-tree changes** (the tree has substantial
> uncommitted work; do NOT `git stash`). Before starting, open each file in
> "Current state" and confirm the quoted excerpts still match. On a mismatch,
> treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: direction
- **Planned at**: commit `d29927d` + uncommitted working tree, 2026-07-04

## Why this matters

VoiceWave's personal dictionary (custom terms the user has taught it) is trapped
in a single encrypted local file. If the user reinstalls or moves machines, the
accumulated dictionary is lost. The `history` module already ships an export
capability; the dictionary — a peer local store — has create/read/delete but no
portability. Export/import is independently useful (backup/migration) and is the
concrete, local-only first step toward the roadmap's opt-in cross-device sync
(`Idea.md:519`). Small, mechanical, low-risk.

## Current state

- `src-tauri/src/dictionary/mod.rs` — the dictionary store and `DictionaryManager`.
  - The store shape (`dictionary/mod.rs:67-73`):
    ```rust
    #[derive(Debug, Clone, Serialize, Deserialize, Default)]
    #[serde(rename_all = "camelCase")]
    struct DictionaryStore {
        next_id: u64,
        queue: Vec<DictionaryQueueItem>,
        terms: Vec<DictionaryTerm>,
    }
    ```
  - `DictionaryTerm` (`dictionary/mod.rs:25-32`): `{ term_id, term, source, created_at_utc_ms }`, serde camelCase.
  - `DictionaryManager` already has `get_terms(&self, query: Option<String>) -> Vec<DictionaryTerm>` (`:246`), `add_term(&mut self, term: &str, source: Option<String>) -> Result<DictionaryTerm, DictionaryError>` (`:272`, which dedupes case-insensitively and returns the existing term if present), and a private `persist(&mut self)` that writes the AES-GCM encrypted file. `add_term` is the correct primitive to reuse for import — it already handles dedupe.
  - `DictionaryError` (`:43-65`) is the error enum; add new variants here if needed (e.g. a parse error for import).
- `src-tauri/src/history/mod.rs` — the export pattern to model after.
  - `HistoryExportResult` (`history/mod.rs:74-80`):
    ```rust
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    #[serde(rename_all = "camelCase")]
    pub struct HistoryExportResult {
        pub preset: HistoryExportPreset,
        pub record_count: usize,
        pub content: String,
    }
    ```
  - `HistoryManager::export_preset(&self, preset, query) -> HistoryExportResult` (`history/mod.rs:311`) returns a `content: String` the frontend then saves. Follow this "backend returns the serialized string; frontend handles the file" convention — do NOT have Rust write files to arbitrary paths.
- `src-tauri/src/state.rs` — the `VoiceWaveController` owns `dictionary_manager: Arc<Mutex<DictionaryManager>>`. Existing controller methods wrap the manager and emit a `voicewave://dictionary` event after mutations. Find `add_dictionary_term` and `get_dictionary_terms` on the controller (grep `fn add_dictionary_term` and `fn get_dictionary_terms` in `state.rs`) and match their shape exactly for the two new controller methods.
- `src-tauri/src/lib.rs` — Tauri command handlers. The dictionary commands are a clean template (`lib.rs:1000-1074`), e.g.:
  ```rust
  #[cfg(feature = "desktop")]
  #[tauri::command]
  async fn add_dictionary_term(
      app: tauri::AppHandle,
      runtime: State<'_, RuntimeContext>,
      term: String,
  ) -> Result<DictionaryTerm, String> {
      runtime.controller.add_dictionary_term(app, term).await
          .map_err(|err| AppError::Controller(err).into())
  }
  ```
  Commands are registered in the `tauri::generate_handler![...]` list (`lib.rs:1206-1211` shows the dictionary entries: `get_dictionary_queue, approve_dictionary_entry, ..., add_dictionary_term`). New commands MUST be added to that list.
- `src/lib/tauri.ts` — the frontend bridge. Dictionary functions live at `tauri.ts:426-453`, e.g.:
  ```ts
  export async function addDictionaryTerm(term: string): Promise<DictionaryTerm> {
    return invokeVoicewave<DictionaryTerm>("add_dictionary_term", { term });
  }
  ```
- `src/hooks/useVoiceWave.ts` exposes dictionary actions (e.g. `addDictionaryTerm`, `deleteDictionaryTerm`) that `App.tsx` consumes. `App.tsx` renders the dictionary view (state around `App.tsx:319` `dictionaryDraftTerm`, and `activeDictionaryTerms` at `App.tsx:465`).

**Conventions to match:**
- Rust error handling uses `thiserror` enums returned as `Result<T, DictionaryError>` at the manager, mapped to `Result<T, String>` at the Tauri command via `AppError::Controller(err).into()`.
- All serde structs crossing the IPC boundary use `#[serde(rename_all = "camelCase")]`.
- Frontend: file open/save uses the Tauri dialog + fs plugins IF already present — CHECK `src-tauri/Cargo.toml` and `src-tauri/capabilities/` for `tauri-plugin-dialog`/`tauri-plugin-fs`. If neither is present, do NOT add a new plugin; instead the export command returns the JSON string and the frontend triggers a browser-style download (Blob + anchor), and import uses a hidden `<input type="file">`. Decide based on what's already available and state which you used.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Rust tests | `cd "C:\Users\posan\OneDrive\Desktop\voice vibe\src-tauri" && export PATH="$PATH:/c/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/Common7/IDE/CommonExtensions/Microsoft/CMake/CMake/bin" && cargo test --lib` | `test result: ok. 218 passed` (or more) |
| Frontend tests | `cd "C:\Users\posan\OneDrive\Desktop\voice vibe" && npx vitest run` | `34 passed` (or more) |
| Frontend build/typecheck | `cd "C:\Users\posan\OneDrive\Desktop\voice vibe" && npm run build` | `✓ built` |

## Scope

**In scope:**
- `src-tauri/src/dictionary/mod.rs` (add `export_terms` + `import_terms` methods + tests)
- `src-tauri/src/state.rs` (add two controller wrappers)
- `src-tauri/src/lib.rs` (add two `#[tauri::command]`s + register them)
- `src/lib/tauri.ts` (add two bridge functions)
- `src/hooks/useVoiceWave.ts` (expose two actions)
- `src/App.tsx` (add Export/Import buttons to the dictionary view)

**Out of scope:**
- `src/lib/cloudSync.ts` and Firebase dictionary sync — cloud sync is a separate, optional path; do not touch it.
- The dictionary `queue` (pending auto-learned terms) — export/import applies to approved `terms` only.
- Any change to the AES-GCM at-rest format or the `.key` file handling.

## Git workflow

- Branch: `advisor/001-dictionary-export-import`
- Commit style: conventional commits, e.g. `feat(dictionary): export/import approved terms`.
- Do NOT push or open a PR unless the operator asks.

## Steps

### Step 1: Add `export_terms` and `import_terms` to `DictionaryManager`

In `src-tauri/src/dictionary/mod.rs`, add a portable export/import format and two methods. Define a versioned envelope near the other structs:
```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryExport {
    pub version: u8,          // = 1
    pub exported_at_utc_ms: u64,
    pub terms: Vec<DictionaryTerm>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryImportSummary {
    pub added: usize,
    pub skipped: usize,   // duplicates ignored
    pub total_in_file: usize,
}
```
Then:
- `pub fn export_terms(&self) -> DictionaryExport` — snapshot `self.store.terms`, stamp `version: 1` and `exported_at_utc_ms: now_utc_ms()`.
- `pub fn import_terms(&mut self, payload: &str) -> Result<DictionaryImportSummary, DictionaryError>` — parse `payload` as `DictionaryExport` via `serde_json::from_str` (map parse failure to `DictionaryError::Parse`). Reject `version != 1` with a clear `DictionaryError` (add a variant like `UnsupportedImportVersion(u8)`). For each imported term, reuse the existing dedupe logic by calling `self.add_term(&term.term, Some("import".to_string()))` — count how many were newly added vs already-present. NOTE: `add_term` returns the existing term on a dupe without indicating whether it was new, so instead check `self.contains_term(&term.term)` (private helper at `dictionary/mod.rs:319`) BEFORE adding to classify added-vs-skipped. `add_term` calls `persist()` per term; that's fine for correctness. Return the summary.

**Verify**: code compiles — `cd src-tauri && cargo build --lib` → exit 0.

### Step 2: Unit-test the manager methods

Add tests to the existing `#[cfg(test)] mod tests` in `dictionary/mod.rs` (find it at the bottom of the file; it already builds managers via a temp path helper — reuse that helper). Cover:
- `export_then_import_round_trips`: add 3 terms, export, construct a fresh manager on a new temp path, import the exported JSON string, assert `added == 3, skipped == 0` and `get_terms(None).len() == 3`.
- `import_skips_duplicates`: manager with terms A,B; import a file containing B,C; assert `added == 1, skipped == 1`.
- `import_rejects_unsupported_version`: hand-craft JSON with `"version": 99`; assert an `Err`.
- `import_rejects_malformed_json`: pass `"{not json"`; assert an `Err`.

**Verify**: `cd src-tauri && cargo test --lib dictionary::` → all dictionary tests pass, including the 4 new ones.

### Step 3: Add controller wrappers in `state.rs`

Find the existing `add_dictionary_term`/`get_dictionary_terms` methods on `VoiceWaveController` in `state.rs` (grep `fn add_dictionary_term`). Add two peers following the SAME shape (lock the `dictionary_manager`, call the manager method, emit the `voicewave://dictionary` event on mutation exactly as the neighbors do):
- `pub async fn export_dictionary(&self) -> DictionaryExport`
- `pub async fn import_dictionary(&self, app: AppHandle, payload: String) -> Result<DictionaryImportSummary, ControllerError>` — emit the dictionary event after a successful import so the UI refreshes term counts.

Import the two new types at the top of `state.rs` where `DictionaryTerm`/`DictionaryQueueItem` are already imported from `crate::dictionary`.

**Verify**: `cd src-tauri && cargo build --lib` → exit 0.

### Step 4: Add Tauri commands + register them

In `lib.rs`, add two `#[cfg(feature = "desktop")] #[tauri::command]` handlers modeled exactly on `add_dictionary_term` (`lib.rs:1062-1074`):
- `export_dictionary(runtime) -> Result<DictionaryExport, String>`
- `import_dictionary(app, runtime, payload: String) -> Result<DictionaryImportSummary, String>`
Add both names to the `tauri::generate_handler![...]` list next to the other dictionary commands (`lib.rs:~1206-1211`). Import `DictionaryExport`/`DictionaryImportSummary` where `DictionaryTerm` is imported in `lib.rs`.

**Verify**: `cd src-tauri && cargo test --lib` → `218 passed` (or more), 0 failed.

### Step 5: Frontend bridge + hook + UI

- `src/lib/tauri.ts`: add `export async function exportDictionary(): Promise<DictionaryExport>` and `importDictionary(payload: string): Promise<DictionaryImportSummary>`, and add the two TS types to `src/types/voicewave.ts` mirroring the Rust structs (camelCase fields). Follow the existing dictionary bridge fns at `tauri.ts:426-453`.
- `src/hooks/useVoiceWave.ts`: expose `exportDictionary` and `importDictionary` actions in the hook's return object next to the existing `addDictionaryTerm`/`deleteDictionaryTerm`. `importDictionary` should refresh the dictionary terms after success (reuse whatever refresh the hook already calls after `addDictionaryTerm`).
- `src/App.tsx`: in the dictionary view (near where `activeDictionaryTerms` is rendered and the add-term input lives), add an **Export** button (calls `exportDictionary`, serializes the result to JSON, and triggers a download of `voicewave-dictionary.json` via a Blob + temporary anchor) and an **Import** button (hidden `<input type="file" accept="application/json">`; on change, read the file text and call `importDictionary(text)`, then surface the summary e.g. "Imported 3 terms (1 skipped)"). Match the existing button styling classes used elsewhere in the dictionary view.

**Verify**: `npx vitest run` → 34 passed (or more); `npm run build` → `✓ built`.

## Test plan

- Rust: 4 new unit tests in `dictionary/mod.rs` (Step 2), modeled after the existing dictionary tests in that file's `mod tests`.
- Frontend: no new required tests (the existing suite must stay green), but if a `tauri.test.ts` pattern exists for command bridges (see `src/lib/tauri.test.ts`), add two assertions that `exportDictionary`/`importDictionary` invoke the right command names — model after the existing dictionary bridge tests there.
- Verification: `cargo test --lib` (218+ pass incl. 4 new) and `npx vitest run` (34+ pass) and `npm run build` (clean).

## Done criteria

- [ ] `cargo test --lib` exits 0 with ≥222 passing (4 new dictionary tests)
- [ ] `npx vitest run` exits 0, ≥34 passing
- [ ] `npm run build` succeeds
- [ ] Export produces a `version: 1` JSON file containing all approved terms
- [ ] Import of that file into a fresh dictionary restores the terms; re-importing skips all as duplicates
- [ ] No files outside the in-scope list modified (`git status`)
- [ ] `plans/README.md` status row updated

## STOP conditions

- The `DictionaryStore`/`DictionaryManager` excerpts in "Current state" don't match the live code.
- No file dialog/fs plugin exists AND the Blob-download / file-input approach is blocked by the app's CSP (check `tauri.conf.json` CSP and `capabilities/`) — if so, report and propose adding `tauri-plugin-dialog` as a follow-up rather than forcing it.
- A verification fails twice after a reasonable fix.

## Maintenance notes

- When cross-device sync (roadmap v1.2) lands, this local export format (`DictionaryExport` v1) should be the interchange schema — bump `version` and add a migration if the term shape changes.
- Reviewer should confirm import is idempotent (re-import skips dupes) and that a malformed/oversized file can't panic the app (parse errors are surfaced, not `unwrap`ped).
- Deferred: exporting/importing the pending `queue` and per-term `source` provenance — approved `terms` only for v1.
