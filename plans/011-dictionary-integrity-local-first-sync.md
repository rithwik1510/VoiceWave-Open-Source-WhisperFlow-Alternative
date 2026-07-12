# Plan 011: Make the personal dictionary local-first, consistent, and safe to sync

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan in
> `plans/README.md` unless a reviewer explicitly says they maintain the index.
>
> **Drift check (run first)**: this plan was written against commit `6b05507`
> **plus the uncommitted working tree present on 2026-07-15**. Do not stash,
> reset, or discard those changes. Run:
>
> ```powershell
> git status --short
> git diff --stat -- src-tauri/Cargo.toml src-tauri/src/dictionary/mod.rs src-tauri/src/state.rs src-tauri/src/lib.rs src/types/voicewave.ts src/lib/tauri.ts src/lib/cloudSync.ts src/hooks/useVoiceWave.ts src/App.tsx src/lib/cloudSync.test.ts src/lib/tauri.test.ts src/App.test.tsx docs/firebase/firestore.rules
> ```
>
> Then compare the "Current state" excerpts below against the live files. The
> worktree already contains unrelated polish-profile and UI changes in several
> in-scope files; preserve them. If an excerpt or named symbol no longer
> matches, treat that as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: L (multi-day; persistence migration + sync + UI + focused tests)
- **Risk**: HIGH (dictionary data and cross-device deletes must not be lost or resurrected)
- **Depends on**: none
- **Blocks**: `plans/004-voice-snippets.md` must be refreshed after this plan lands
- **Category**: correctness + security + architecture
- **Planned at**: commit `6b05507` plus the 2026-07-15 working tree, 2026-07-15

## Why this matters

VoiceWave currently has two dictionaries that look like one. Signed-out users
mutate the encrypted Rust/local dictionary used by transcription; signed-in
users mutate a Firebase collection displayed by React, while the Rust
transcription path continues reading only its local store. A term can therefore
appear saved, synced, deleted, or exported in the UI without that action
matching what the ASR worker actually uses.

This plan makes the encrypted local dictionary the single runtime source of
truth in every account state. Firebase becomes optional, eventually consistent
replication: local mutations succeed offline first, then synchronize when
possible. The plan also removes the plaintext migration-backup leak, prevents
unapproved suggestions from biasing ASR, validates approval through the same
dedupe path as manual additions, caps the pending queue, and exposes correction
editing plus honest sync status in the existing Quiet Ink UI.

When this lands, voice snippets can reuse a proven local-first replication
boundary without inheriting the current split-brain behavior.

## Product contract

These are requirements, not suggestions:

1. **Local is canonical.** Dictation always reads the encrypted local store,
   whether the user is signed out, signed in, online, or offline.
2. **Cloud is replication, never an alternate UI collection.** Signing in
   reconciles remote records into the local manager; the UI continues rendering
   the local terms returned by Rust.
3. **Local mutations do not wait for cloud.** Add, approve, edit, remove, and
   import persist locally first. A cloud failure changes sync status but never
   rolls back a successful local mutation.
4. **Deletes propagate.** Use tombstones and deterministic cloud document IDs;
   do not use union-only merging, because it resurrects deleted terms.
5. **Existing data is preserved.** Merge local and remote terms on first
   signed-in reconciliation. Never replace one entire side with the other.
6. **Legacy cloud rows are migrated safely.** Write the deterministic record
   before deleting a legacy random-ID document.
7. **Pending suggestions are untrusted.** They never enter the ASR terminology
   prompt until approved.
8. **Exports remain portable.** Export active local terms only; do not export
   tombstones, cloud IDs, or sync metadata.

## Current state

### Runtime/local path

- `src-tauri/src/dictionary/mod.rs` owns the encrypted JSON store. Its current
  stored shape is `{ next_id, queue, terms }`; `DictionaryTerm` has only
  `term_id`, `term`, `source`, and `created_at_utc_ms`.
- `DictionaryManager::add_term` trims, rejects only empty strings, deduplicates
  with ASCII-only case comparison, removes a matching queue item, and persists.
- `DictionaryManager::approve_entry` removes a queue item and pushes a new term
  directly. It does **not** reuse `add_term`, so an empty normalized value or a
  duplicate approved value can be stored.
- `DictionaryManager::load` detects legacy plaintext, calls
  `backup_legacy_plaintext`, and then encrypts the primary file. The helper
  copies plaintext to `dictionary.json.bak`, leaving the user's vocabulary
  readable after migration.
- `DictionaryManager::ingest_transcript_with_signal` and
  `queue_correction_candidates` append suggestions without a store-wide queue
  cap or expiry.

Relevant excerpts:

```rust
// src-tauri/src/dictionary/mod.rs:75-80
struct DictionaryStore {
    next_id: u64,
    queue: Vec<DictionaryQueueItem>,
    terms: Vec<DictionaryTerm>,
}
```

```rust
// src-tauri/src/dictionary/mod.rs:221-248
pub fn approve_entry(...) -> Result<DictionaryTerm, DictionaryError> {
    // ...remove queue row...
    let term = DictionaryTerm {
        term_id: self.next_id("dt"),
        term: normalized_text.unwrap_or_else(|| entry.term).trim().to_string(),
        source: "queue-approval".to_string(),
        created_at_utc_ms: now_utc_ms(),
    };
    self.store.terms.push(term.clone());
    self.persist()?;
    Ok(term)
}
```

```rust
// src-tauri/src/dictionary/mod.rs:415-423
if let Ok(encrypted) = serde_json::from_str::<EncryptedDictionaryStore>(&raw) {
    self.store = decrypt_dictionary_store(&encrypted, &self.key)?;
} else {
    self.store = serde_json::from_str(&raw).map_err(DictionaryError::Parse)?;
    backup_legacy_plaintext(&self.path)?;
    self.persist()?;
}
```

### ASR use

- `src-tauri/src/state.rs::build_inference_worker` builds faster-whisper's
  terminology hint from pending queue rows, environment terms, and approved
  local terms.
- The builder selects at most 10 strings, reading the assembled list in
  reverse. Approved terms win first, but pending/unapproved guesses fill unused
  slots.
- `src-tauri/src/transcript/mod.rs::stabilize_custom_terms` reapplies exact
  dictionary spellings case-insensitively after decoding. This helps casing but
  cannot repair arbitrary phonetic substitutions; do not claim otherwise in UI
  copy.

```rust
// src-tauri/src/state.rs:4566-4578
let hint = {
    let manager = self.dictionary_manager.lock().await;
    let mut terms = manager
        .get_queue(Some(12))
        .into_iter()
        .map(|row| row.term)
        .collect::<Vec<_>>();
    terms.reverse();
    terms.extend(env_technical_terms());
    terms.extend(manager.get_terms(None).into_iter().map(|row| row.term));
    build_terminology_hint_from_texts(&terms, 10)
};
```

### Split cloud/UI path

- `src/App.tsx` holds `cloudDictionaryTerms` separately and chooses it whenever
  `cloudUserId` exists:

```tsx
// src/App.tsx:607
const activeDictionaryTerms = cloudUserId ? cloudDictionaryTerms : dictionaryTerms;
```

- Signed-in manual add, delete, and queue approval call
  `addCloudDictionaryTerm` / `deleteCloudDictionaryTerm` directly and skip the
  local Tauri mutation.
- Export/import always call the local Tauri commands. Consequently, a signed-in
  user can see one list and export another.
- `src/lib/cloudSync.ts` stores random-ID Firestore documents with
  `{ term, source, termNormalized, createdAtUtcMs }`; it has no updated time or
  tombstone.
- `docs/firebase/firestore.rules` permits exactly those four fields for
  dictionary documents.
- `src/App.test.tsx` tests demo sign-in but has no signed-in dictionary mutation
  or reconciliation test. `src/lib/cloudSync.test.ts` tests auth and sentence
  writes only.

### Conventions to match

- Rust persistence errors use `thiserror`; controller methods map manager errors
  through `ControllerError`; Tauri handlers return `Result<T, String>`.
- Rust/TypeScript IPC payloads use serde camelCase.
- Cloud functions use `CloudSyncError`, `withCloudRetry`, client backpressure,
  and lazy Firebase imports in `src/lib/cloudSync.ts`.
- Frontend bridge functions live in `src/lib/tauri.ts`; stateful actions live in
  `src/hooks/useVoiceWave.ts`; `src/App.tsx` renders the dictionary view.
- UI changes must reuse existing Quiet Ink classes (`vw-field`,
  `vw-btn-primary`, `vw-btn-secondary`, `vw-row-list`, muted Zinc text). Do not
  introduce a second visual system.

## Target data model and merge contract

Keep public `DictionaryTerm` backward-compatible for the UI and export format.
Add separate internal/sync records rather than leaking tombstones into normal
term lists.

### Rust internal record

In `src-tauri/src/dictionary/mod.rs`, replace `DictionaryStore.terms`' internal
element type with a private backward-compatible record:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct StoredDictionaryRecord {
    term_id: String,
    term: String,
    normalized_term: String,
    source: String,
    created_at_utc_ms: u64,
    updated_at_utc_ms: u64,
    deleted_at_utc_ms: Option<u64>,
}
```

Implement an explicit migration from existing `DictionaryTerm` rows. Do not
depend on a derived `Default` silently producing empty required fields. Active
records map to the existing public `DictionaryTerm`; tombstones never appear in
`get_terms` or exports.

### IPC/cloud sync record

Add a public Rust and TypeScript `DictionarySyncRecord`:

```text
term: string
normalizedTerm: string
source: string
createdAtUtcMs: number
updatedAtUtcMs: number
deletedAtUtcMs: number | null
```

Cloud document ID must be deterministic:

```ts
`term-${encodeURIComponent(normalizedTerm)}`
```

The prefix avoids Firestore's special `.` / `..` document names. Never use a
raw term as an unescaped path segment.

### Normalization

Use one documented normalization contract on both sides:

1. Trim leading/trailing whitespace.
2. Collapse internal whitespace runs to one ASCII space.
3. Normalize Unicode to NFC.
4. Lowercase for identity; preserve the user's display spelling separately.

Use the Rust `unicode-normalization` crate and JavaScript
`value.normalize("NFC").toLowerCase()`. Add the dependency only to
`src-tauri/Cargo.toml`. Valid active terms are 1–72 Unicode scalar values and
must not contain control characters or line breaks.

### Reconciliation

Implement reconciliation inside `DictionaryManager`, not independently in
React. For each normalized identity:

1. Compare local and remote `updatedAtUtcMs` / `deletedAtUtcMs` effective
   timestamps.
2. Newer record wins.
3. On equal timestamps, deletion wins over an active record.
4. On an exact active tie, keep local display spelling to avoid churn.
5. Persist the merged local store once, not once per record.
6. Return active `terms` plus the complete winning records that must be upserted
   to cloud.

Tombstones remain stored and synced; do not prune them in this plan. The
dictionary is bounded and retaining them prevents an old offline device from
resurrecting deletions.

**Monotonic timestamp guard (clock-skew mitigation).** On every local
mutation (add, approve, edit, remove, import), set
`updated_at_utc_ms = max(now_utc_ms(), existing.updated_at_utc_ms + 1)`.
This guarantees a user's newer local action always outranks their own older
state even if the device clock stepped backwards. Cross-device clock skew
remains a known last-writer-wins limitation (see maintenance notes); do not
build vector clocks in this plan.

**Identity fields.** `term_id` is device-local and is never synced — two
devices will hold different `term_id`s for the same term, and that is
correct. The only cross-device identity is `normalizedTerm`. Do not "fix"
this by syncing IDs.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Focused Rust dictionary tests | `cargo test --no-default-features --manifest-path src-tauri/Cargo.toml dictionary::` | exit 0; all dictionary tests pass |
| Focused Rust terminology-hint tests | `cargo test --no-default-features --manifest-path src-tauri/Cargo.toml terminology` | exit 0; all matching tests pass |
| Focused frontend tests | `npx vitest run src/lib/cloudSync.test.ts src/lib/dictionarySync.test.ts src/lib/tauri.test.ts src/App.test.tsx` | exit 0; all selected tests pass |
| Firestore policy check | `npm run security:firestore-rules -- -Enforce` | exit 0 |
| Frontend build/typecheck | `npm run build` | exit 0 |
| Desktop compile gate | `cargo test --manifest-path src-tauri/Cargo.toml --no-run` | exit 0 |
| Scope check | `git status --short` | only pre-existing changes plus plan in-scope edits |

Do not run package installation, formatting across the repo, the full release
gate, or generated artifact scripts for this plan.

## Scope

**In scope:**

- `src-tauri/Cargo.toml` — add only Unicode normalization dependency.
- `src-tauri/src/dictionary/mod.rs` — internal records, migration, validation,
  tombstones, reconciliation, queue cap, encrypted persistence behavior, tests.
- `src-tauri/src/state.rs` — controller sync methods and approved-only ASR hint.
- `src-tauri/src/lib.rs` — sync commands and handler registration.
- `src/types/voicewave.ts` — sync record/result types.
- `src/lib/tauri.ts` — sync bridge commands.
- `src/lib/cloudSync.ts` — deterministic cloud records and legacy-row migration.
- `src/lib/dictionarySync.ts` (new) — cloud/Tauri reconciliation orchestration.
- `src/hooks/useVoiceWave.ts` — local state refresh and sync action.
- `src/App.tsx` — local-only rendering/mutations, sign-in reconciliation,
  editable approval, search, sync status/retry.
- `src-tauri/src/dictionary/mod.rs` tests in the same module.
- `src/lib/cloudSync.test.ts`, `src/lib/dictionarySync.test.ts` (new),
  `src/lib/tauri.test.ts`, `src/App.test.tsx` — focused regression coverage.
- `docs/firebase/firestore.rules` — sync record/tombstone schema.
- `plans/README.md` — status only when execution completes.

**Out of scope:**

- Voice snippet storage, matching, expansion, or UI. Refresh plan 004 only
  after this foundation is merged.
- Dictionary aliases/phonetic variants, automatic keyboard-correction capture,
  usage ranking, app-scoped terms, team/shared dictionaries, or multilingual
  quality claims.
- Moving encryption keys to DPAPI/Windows Credential Manager. History and
  diagnostics share the current sibling-key convention; migrate them together
  in a separate security plan.
- History or recent-sentence cloud-sync behavior.
- Changes to Firebase authentication or account/profile UX.
- A general `state.rs`, `useVoiceWave.ts`, or `App.tsx` refactor.
- Monetization or Pro gating.

## Git workflow

- Branch: `codex/011-dictionary-local-first-sync`
- Preserve all pre-existing worktree changes. Never stash, reset, or revert
  files wholesale.
- Commit style: conventional commit, e.g.
  `fix(dictionary): make cloud sync local-first`.
- Do not push or open a pull request unless the operator explicitly requests it.

## Steps

### Step 1: Characterize the current local and split-brain behavior

Before changing production code, add failing/characterization tests that prove:

- Manual local add deduplicates case-insensitively.
- `approve_entry` currently needs the same empty/duplicate protection as add.
- Legacy plaintext migration must not leave plaintext in any backup artifact.
- Pending suggestions must not enter the terminology hint.
- Signed-in add/delete/approval must invoke the local Tauri mutation before
  cloud synchronization.
- Export uses the same local collection rendered by the UI.

Use existing test structure in `dictionary/mod.rs`, `cloudSync.test.ts`,
`tauri.test.ts`, and `App.test.tsx`; do not add an E2E framework.

**Verify**: run the focused Rust and frontend commands. Tests that describe the
new contract should fail for the expected old behavior; existing unrelated
tests must remain green. Record the expected failing test names in the commit or
working notes before proceeding.

### Step 2: Harden local dictionary records and validation

In `dictionary/mod.rs`:

1. Introduce private `StoredDictionaryRecord` plus explicit migration from the
   current active-term rows.
2. Centralize normalization/validation in one helper used by manual add,
   approval, import, and reconciliation.
3. Make approval call the same internal upsert path as manual add. Approving a
   duplicate removes its queue item and returns the existing active term;
   approving empty/invalid input returns a typed validation error without losing
   the queue item.
4. Make remove create/update a tombstone rather than physically forgetting the
   identity.
5. Enforce `MAX_APPROVED_TERMS = 1000`, `MAX_PENDING_TERMS = 50`, and a 72-character
   display-term limit. When adding suggestions beyond the pending cap, retain
   the newest high-signal items; never silently delete approved records.
   **Tombstones do not count against `MAX_APPROVED_TERMS`** — the cap applies
   to active records only, otherwise a heavy add/delete churner locks
   themselves out of new adds.
6. Filter tombstones from `get_terms`, ASR term collection, normal events, and
   exports.
7. Import v1 files through the central upsert path and persist once after the
   batch. Preserve the existing import result counts.

Do not add aliases, priority fields, or usage tracking here.

**Verify**: `cargo test --no-default-features --manifest-path src-tauri/Cargo.toml dictionary::` → all dictionary tests pass, including new validation, duplicate approval, tombstone, cap, migration, and v1 import cases.

### Step 3: Remove plaintext migration residue

Delete `backup_legacy_plaintext` and its call. Legacy migration must:

1. Parse the legacy plaintext into memory.
2. Normalize/migrate its records.
3. Persist the encrypted envelope successfully.
4. Re-read and decrypt the new primary file in the test.
5. Confirm neither the primary file nor any `.bak`/temporary artifact contains
   a known test term in plaintext.

If preserving a recovery backup is necessary, create it only from the encrypted
envelope after successful encryption and name it clearly as encrypted. Never
copy the plaintext source.

**Verify**: the dedicated migration test passes and
`rg -n "backup_legacy_plaintext" src-tauri/src/dictionary/mod.rs` returns no matches.

### Step 4: Add Rust-owned reconciliation and IPC commands

Add public sync payloads and manager methods:

- `get_dictionary_sync_records()` — all active records and tombstones.
- `reconcile_dictionary_records(remote_records)` — apply the merge contract,
  persist once, and return:
  - current active public terms;
  - winning sync records that cloud must upsert.

Add controller wrappers in `state.rs`, Tauri commands in `lib.rs`, registrations
in `generate_handler!`, TypeScript payloads in `voicewave.ts`, and bridge
functions in `tauri.ts`.

The Rust manager owns normalization and winner selection. TypeScript may
normalize only to compute deterministic document IDs and must assert that its
result matches the Rust-provided `normalizedTerm`; it must not independently
decide merge winners.

Add tests for local-newer, remote-newer, deletion-newer, deletion tie, active
tie, remote-only, local-only, and idempotent second reconciliation.

**Verify**: focused Rust sync tests and `src/lib/tauri.test.ts` pass.

### Step 5: Replace cloud CRUD with deterministic sync records

In `cloudSync.ts`:

1. Replace dictionary-specific add/delete-as-source-of-truth functions with:
   - `listCloudDictionaryRecords(uid)`;
   - `upsertCloudDictionaryRecords(uid, records)`;
   - legacy random-ID cleanup performed only after deterministic writes succeed.
2. Use document ID `term-${encodeURIComponent(normalizedTerm)}`.
3. Store all sync fields, including `updatedAtUtcMs` and nullable
   `deletedAtUtcMs`.
4. Retain `CloudSyncError`, retry, owner scoping, and lazy SDK imports.
5. **Bulk upsert mechanics** (first sign-in can carry up to 1000 records —
   the existing per-write backpressure was designed for single manual adds
   and will throttle or fail a naive loop):
   - Write via Firestore `writeBatch` in chunks of at most 500 operations.
   - Treat one reconciliation run as **one** backpressured operation
     (`enforceClientBackpressure` once per sync run, not per record).
   - A partially-completed upsert must be safe to re-run: deterministic IDs
     make every chunk idempotent, so on failure simply retry the whole run.
6. Map legacy four-field documents to active sync records with
   `updatedAtUtcMs = createdAtUtcMs` and `deletedAtUtcMs = null`.
7. Never delete a legacy row before the deterministic replacement write exits
   successfully.

Create `src/lib/dictionarySync.ts` as a small orchestrator:

```text
fetch remote records
  -> invoke Rust reconciliation
  -> upsert Rust's winning records
  -> clean up migrated legacy IDs
  -> return active local terms
```

Do not put merge policy in this TypeScript module.

Update `docs/firebase/firestore.rules` to permit exactly the new schema and
validate what rules *can* check: owner, field types, lengths,
created/updated timestamps, and a nullable deletion timestamp
(`deletedAtUtcMs is int || deletedAtUtcMs == null`). Rules **cannot**
compute `encodeURIComponent`, so do not attempt to validate that the
document ID matches `normalizedTerm` in rules — ID↔identity consistency is
enforced by the client assertion against Rust's `normalizedTerm` (Step 4).
Existing legacy records must remain readable during migration.

**Verify**: cloud/dictionary-sync tests pass and
`npm run security:firestore-rules -- -Enforce` exits 0.

### Step 6: Make every UI mutation local-first

In `useVoiceWave.ts` expose:

- a focused `refreshDictionary()` that refreshes queue and local terms without
  reloading models/history/benchmarks;
- `syncDictionaryWithCloud(uid)` using `dictionarySync.ts` and updating local
  state from the returned active terms.

In `App.tsx`:

1. Remove `cloudDictionaryTerms` as an alternate rendered collection.
   `dictionaryTerms` is always the displayed approved list.
2. Reconcile cloud records into local on **every auth-ready event** — not
   only interactive sign-in. That includes app restart while already signed
   in (`onAuthStateChanged` firing with an existing user) and a successful
   `Sync pending` retry. Otherwise a user who mutated offline and restarts
   stays stale until they manually retry. Never replace local terms with the
   fetched cloud array.
3. The pending suggestion queue is local-only and **never syncs** — the sync
   record covers approved/tombstoned terms exclusively.
4. Manual add, removal, approval, and import always await the local action
   first. When signed in, trigger synchronization afterward.
5. A cloud failure leaves the local success visible and shows `Sync pending`
   with a Retry action. It must not show the local mutation as failed.
6. Signing out retains the local merged dictionary and changes only the sync
   status.
7. Export always represents the exact active local collection shown on screen.
8. Remove misleading copy that says a Firebase-only write immediately affects
   every install.

Avoid broad auth or layout changes.

**Verify**: focused App tests prove local-first ordering, offline success,
sign-in merge, sign-out retention, retry status, and export consistency.

### Step 7: Make pending review editable and bounded

In the existing pending-review UI:

- Render an editable term field for each pending suggestion, initialized from
  the suggestion.
- Approve with the edited value via the already-supported `normalizedText`
  argument.
- Preserve the queue row and show the backend error if validation fails.
- Add a local client-side search field for approved terms. Search the displayed
  term and source; do not issue a cloud query.
- Show one restrained sync label: `Device local`, `Synced`, `Sync pending`, or
  `Syncing`. Reuse existing Quiet Ink classes.

Do not add pinning, aliases, usage analytics, or a microphone test in this plan.

**Verify**: focused App tests cover edited approval, invalid approval retention,
and search filtering; `npm run build` exits 0.

### Step 8: Remove pending suggestions from ASR hints

In `state.rs::build_inference_worker`, construct the terminology hint from:

1. Environment technical terms (lower priority).
2. Active approved local dictionary terms (higher priority).

Do not include `get_queue()` results. Keep the existing limit of 10 for this
integrity plan; smarter ranking is deferred. Add/adjust tests demonstrating
that approved terms win and a queue-only term is absent.

**Verify**: focused Rust terminology tests pass.

### Step 9: Run the complete focused gate and inspect scope

Run, in order:

1. Focused Rust dictionary/sync tests.
2. Focused frontend tests.
3. Firestore policy check.
4. `npm run build`.
5. Desktop compile gate.
6. `git diff --check`.
7. `git status --short` and `git diff --stat`.

Do not report completion if any gate fails. Update only Plan 011's status row in
`plans/README.md` when all gates pass.

## Test plan

### Rust: `src-tauri/src/dictionary/mod.rs`

Add focused tests for:

- Existing encrypted store loads and migrates without data loss.
- Legacy plaintext migrates with no plaintext backup residue.
- Unicode NFC + lowercase normalization deduplicates canonical equivalents.
- Empty, control-character, newline, overlength, and over-cap terms reject.
- Manual add and queue approval share dedupe behavior.
- Invalid edited approval keeps the queue row.
- Pending queue caps at 50 and keeps newest valid suggestions.
- Removal creates an invisible tombstone; re-add with a newer timestamp
  resurrects intentionally.
- Local mutation timestamps are monotonic: a mutation after a clock
  step-backwards still produces `updated_at_utc_ms` strictly greater than the
  record's previous value.
- Tombstones do not count against the 1000 active-term cap.
- Export excludes tombstones and remains v1-import compatible.
- Reconciliation winner matrix: local/remote/newer/tie/deletion/idempotence.

### Rust: `src-tauri/src/state.rs`

- Approved terms enter the terminology hint.
- Pending-only terms never enter the hint.
- Existing environment-term priority remains stable.

### TypeScript

- `cloudSync.test.ts`: deterministic IDs, legacy migration write-before-delete,
  tombstone payload, retry/non-retry behavior.
- `dictionarySync.test.ts`: call ordering and no merge decisions in TS.
- `tauri.test.ts`: new command names and camelCase arguments.
- `App.test.tsx`: signed-in add/delete/approve call local first; cloud failure
  preserves local success; sign-in merges; sign-out retains; export/display
  consistency; edited approval; search.

Use mocks only at Tauri/Firebase boundaries. Do not mock the pure normalization
or reconciliation functions under test.

## Done criteria

- [ ] The dictionary rendered in the UI is always `dictionaryTerms` from the
  local Rust manager; there is no `cloudUserId ? cloudDictionaryTerms : ...`
  alternate collection.
- [ ] Signed-in add, remove, approval, and import persist locally before any
  Firebase call.
- [ ] A failed Firebase call leaves the local mutation active and exposes a
  retryable `Sync pending` state.
- [ ] First sign-in merges local and remote records without wholesale
  replacement or data loss.
- [ ] Tombstones prevent a deleted term from returning after reconciliation.
- [ ] Cloud documents use deterministic normalized IDs; legacy rows are deleted
  only after replacement writes succeed.
- [ ] Reconciliation upserts go through chunked `writeBatch` writes (≤500 ops)
  with backpressure applied once per sync run, not per record.
- [ ] Reconciliation runs on every auth-ready event (including app restart
  while signed in), not only interactive sign-in.
- [ ] Pending suggestions never appear in faster-whisper terminology hints.
- [ ] Approval uses shared validation/dedupe and supports an edited value.
- [ ] Pending queue is capped at 50; approved active terms are capped at 1000.
- [ ] Plaintext migration leaves no plaintext `.bak` or temporary artifact.
- [ ] Export contains the same active local terms shown in the UI and excludes
  tombstones/sync metadata.
- [ ] Focused Rust tests, focused frontend tests, Firestore rule check,
  `npm run build`, and desktop compile gate all exit 0.
- [ ] `git diff --check` exits 0.
- [ ] No out-of-scope files were modified by the executor.
- [ ] Plan 011 status in `plans/README.md` is updated only after all gates pass.

## STOP conditions

Stop and report instead of improvising if:

- Any current-state excerpt or named function has materially changed.
- Preserving existing local/cloud data appears to require destructive reset,
  collection deletion, or dropping the encrypted store.
- Firestore deployment would reject legacy documents before the client has a
  migration path that can read them.
- A deterministic normalized ID would exceed Firestore document-ID limits for
  the enforced 72-character term contract.
- Reconciliation cannot be made idempotent in tests.
- A cloud failure would require rolling back a successful local mutation.
- The implementation requires moving Firebase credentials or cloud access into
  Rust/Tauri. This plan keeps Firebase in the existing frontend boundary.
- A fix appears to require changing history, recent-sentence sync, auth, billing,
  or voice-snippet behavior.
- An in-scope file contains overlapping user changes that cannot be preserved
  with a narrow patch.
- Any verification command fails twice after one reasonable correction.

## Maintenance notes

- Review the reconciliation tests more closely than the UI. Data resurrection
  and silent replacement are the highest-risk failure modes.
- **Version-rollback tradeoff (deliberate).** Old binaries deserialize the new
  store fine because serde ignores unknown fields — which means on a version
  downgrade, tombstoned records silently reappear as active local terms
  (`deleted_at_utc_ms` is dropped). This is accepted: the alternative (bumping
  the encrypted envelope version so old code fails closed) would brick the
  dictionary entirely on downgrade, which is worse. Keep envelope version 1.
  The old cloud CRUD path no longer exists post-plan, so resurrected terms
  stay local until the user upgrades again and the next reconciliation
  re-applies the (still newer) cloud tombstones.
- **Clock skew is a known LWW limitation.** The monotonic guard protects a
  device against its own clock stepping backwards, but a device whose clock
  runs behind another device's can still lose cross-device races (e.g. its
  delete losing to an older-in-reality active record). Accepted for v1; if it
  bites in practice, revisit with a per-record change counter, not wall time.
- Local encryption still uses a sibling key file, matching history and
  diagnostics. That protects against casual plaintext exposure but is not an OS
  credential-store boundary; migrate all three stores together later.
- The ASR hint remains capped at 10 approved terms. A later dictionary-quality
  plan may add pinned terms, usage ranking, aliases, and app-aware selection.
- Voice snippets should reuse the local-first sync orchestration and
  deterministic record identity, but keep a separate domain model. Snippet
  expansions must be protected from formatting/LLM rewrites.
- If cloud schema changes again, keep legacy readers until telemetry or an
  explicit migration window proves old records are gone.
