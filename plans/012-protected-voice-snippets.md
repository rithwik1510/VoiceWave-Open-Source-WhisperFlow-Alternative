# Plan 012: Ship protected, local-first voice snippets

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update this plan's status row in
> `plans/README.md` unless a reviewer explicitly says they maintain the index.
>
> **Drift check (run first)**: this plan was written against commit `6b05507`
> **plus the uncommitted working tree present on 2026-07-16**. That tree already
> contains Plan 011 and polish-profile work. Do not stash, reset, discard, or
> reimplement those changes. Run:
>
> ```powershell
> git status --short
> git diff --stat -- src-tauri/src/dictionary/mod.rs src-tauri/src/lib.rs src-tauri/src/state.rs src-tauri/src/transcript/mod.rs src/prototype/constants.ts src/types/voicewave.ts src/lib/tauri.ts src/lib/cloudSync.ts src/lib/dictionarySync.ts src/hooks/useVoiceWave.ts src/App.tsx src/App.test.tsx src/lib/tauri.test.ts src/lib/cloudSync.test.ts docs/firebase/firestore.rules scripts/security/check-firestore-rules.ps1
> ```
>
> Compare the "Current state" section with the live files. Preserve unrelated
> work in every shared file. If a named symbol or pipeline order has materially
> changed, treat that as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: L (multi-day: protected runtime expansion + encrypted CRUD + sync + UI)
- **Risk**: HIGH (a bad implementation can alter private saved text, leak it to polish, or resurrect deleted snippets)
- **Depends on**: `plans/011-dictionary-integrity-local-first-sync.md` (DONE)
- **Supersedes**: `plans/004-voice-snippets.md`
- **Category**: direction + correctness + security + architecture
- **Planned at**: commit `6b05507` plus the 2026-07-16 working tree, 2026-07-16

## Why this matters

Voice snippets are the missing high-frequency automation layer: a user says a
memorable phrase such as "my support reply" and VoiceWave inserts an exact saved
email, address, link, signature, or reusable paragraph. Current competitors
treat this as a core workflow, including inline triggers, exact expansion text,
search/edit/delete, and cross-device availability.

The old Plan 004 is unsafe against the product that exists now. It expands the
saved text before deterministic formatting and AI polish, copies the dictionary
encryption implementation, and is local-only. That would allow VoiceWave to
rewrite user-owned content and recreate the split local/cloud state that Plan
011 just removed.

This plan makes snippets a separate first-class domain while reusing Plan 011's
local-first replication contract. Spoken triggers are detected before formatting,
but their expansions remain opaque and never enter deterministic transforms or
the local LLM. The exact expansion is restored only at the final delivery
boundary.

## Product contract

These are requirements, not suggestions:

1. **A snippet is not a dictionary term.** Dictionary terms stabilize spelling;
   snippets replace a spoken phrase with user-owned text. They have separate
   storage, commands, UI, sync records, limits, and telemetry.
2. **Local is canonical.** Snippets work signed out and offline. Every add,
   edit, rename, and delete persists to the encrypted Rust store first; cloud
   replication follows when available and never rolls back a local success.
3. **Saved expansions are literal.** Preserve expansion casing, punctuation,
   newlines, indentation, URLs, and symbols byte-for-byte. Do not sentence-case,
   code-format, terminology-stabilize, or LLM-polish the expansion.
4. **Expansion text never enters the LLM.** Inline dictation may be polished only
   with opaque placeholders. A candidate that drops, duplicates, invents, or
   changes a placeholder is rejected and falls back to the deterministic path.
5. **Exact-only triggers are instant.** If the utterance is only a trigger
   (allowing ASR-added terminal punctuation), insert the expansion directly and
   skip deterministic formatting and all polish work.
6. **Inline triggers work.** "Send this to my work email tomorrow" expands the
   matching phrase in place. Multiple and repeated snippets in one dictation are
   supported.
7. **Matching is predictable.** Match case-insensitively after Unicode NFC and
   whitespace normalization, only at Unicode word/phrase boundaries. Resolve
   overlaps longest-trigger-first, then left-to-right. Never match inside a
   larger word.
8. **Identity is normalized trigger text.** Two active snippets cannot have the
   same normalized trigger. Editing a trigger is an atomic rename: tombstone the
   old identity and create/update the new identity in one local persist.
9. **Deletes propagate without retaining sensitive content.** Tombstones retain
   the normalized trigger and timestamps but clear the expansion. A newer
   explicit re-add may resurrect that identity.
10. **No internal placeholder escapes.** Events, logs, clipboard/pill offers,
    insertion history, correction learning, and UI state must contain either the
    spoken source or restored text, never protection tokens.
11. **Existing dictation quality signals remain honest.** ASR/no-speech integrity
    is evaluated against the spoken pre-expansion transcript. Snippet-involved
    dictations are excluded from automatic dictionary correction derivation so a
    large canned expansion cannot be learned as a "correction."
12. **No new paywall in this plan.** Follow the app's current entitlement policy.
    Do not add billing or plan-gating logic while Pro remains included.

### V1 limits and validation

- Trigger: 1–60 Unicode scalar values after trimming; collapse internal
  whitespace for identity and matching while retaining the user's display form.
- Expansion: 1–4,000 Unicode scalar values; preserve leading/internal/trailing
  content after converting CRLF to LF for cross-platform determinism. The UI
  must show a live count and reject an over-limit save before IPC; Rust remains
  authoritative.
- Active snippets: maximum 250 per device/account after reconciliation.
  Tombstones do not count.
- Expansions per dictation: maximum 16 matched occurrences. If a seventeenth
  match would occur, do not partially expand; insert the original deterministic
  transcript and surface a non-sensitive diagnostic reason.
- Reject an exact trigger that collides with an enabled built-in structural
  command (`new line`, `next line`, `new paragraph`, `bullet point`, `new bullet`).
  Keep this reserved list next to the transcript command vocabulary and test it.
- A one-word or very short trigger is allowed but the UI warns that common
  phrases may expand accidentally. Do not silently change it or require a
  magic suffix.

## Market baseline used for scope

Checked 2026-07-16 against official product documentation:

- Wispr Flow supports whole-word, case-insensitive inline trigger matching,
  exact expansion casing, full-trigger dictation with an auto-added period,
  60-character triggers, 4,000-character expansions, and searchable CRUD.
  Source: <https://docs.wisprflow.ai/articles/5784437944-create-and-use-snippets>
- Wispr surfaces Snippets as a separate desktop navigation destination rather
  than hiding them under Dictionary or Settings.
  Source: <https://docs.wisprflow.ai/articles/5096240724-navigating-the-wispr-flow-app-desktop-ios-and-android>
- Willow syncs spoken shortcuts across devices and positions them for emails,
  templates, links, and addresses.
  Source: <https://help.willowvoice.com/en/articles/13183918-using-personal-dictionary-and-shortcuts>

V1 must meet that personal-snippet baseline. Shared/team snippets, bulk sharing,
and dynamic variables are explicitly deferred until the private local-first
core is proven.

## Current state

### Transcript and delivery pipeline

- `src-tauri/src/transcript/mod.rs::finalize_pro_transcript` currently runs:
  sanitize/filler pruning → user finalization → structural commands → domain
  corrections → dictionary stabilization → format profile → app profile → code
  mode → whitespace normalization.
- `finalize_literal_transcript` still performs structural commands, dictionary
  stabilization, and punctuation; it is not safe to run an expansion through it.
- `apply_code_mode` may convert a short 2–8 word transcript to camel/snake/Pascal
  case. A naive textual sentinel can therefore be mutated.
- `replace_boundary_phrase_case_insensitive` is ASCII-oriented. Do not reuse it
  as the snippet matcher; snippet identity and boundaries must be Unicode-aware.

```rust
// src-tauri/src/transcript/mod.rs:33
pub fn finalize_pro_transcript(input: &str, options: &ProTranscriptOptions<'_>) -> String {
    // ...structural commands, corrections, profiles, code mode...
}
```

- `src-tauri/src/state.rs::run_dictation_flow` resolves the insert path, loads
  dictionary terms once, creates `final_transcript`, optionally runs immediate,
  wait-validated, background, or late polish, then emits/inserts/records
  `inserted_text`.
- Both the blocking and asynchronous polish paths call `gate_polish_candidate`.
  Snippet placeholder validation must wrap every accepted candidate, not only
  the first wait-validated branch.
- `correction_session` currently compares prior and current inserted text to
  derive dictionary candidates. Expanded text must not participate.

```rust
// src-tauri/src/state.rs:3834
let mut inserted_text = final_transcript.clone();

// src-tauri/src/state.rs:3892
match gate_polish_candidate(&final_transcript, &candidate, &custom_terms, polish_profile) {
    // accepted text can become inserted_text
}

// src-tauri/src/state.rs:3994-4001
let previous_correction_session = { self.correction_session.lock().await.clone() };
// derive_correction_candidates(&previous.inserted_text, &inserted_text)
```

### Local-first dictionary precedent

- `src-tauri/src/dictionary/mod.rs` now owns normalized identity, encrypted local
  persistence, timestamps/tombstones, deterministic reconciliation, and public
  sync records that exclude local IDs.
- `src/lib/dictionarySync.ts` implements fetch remote → Rust reconcile → write
  winners → clean legacy rows. `src/App.tsx` renders local Rust state in every
  account mode and represents cloud as `device-local | syncing | synced | pending`.
- `src/lib/cloudSync.ts` uses deterministic document IDs, batched writes, content
  hashes, and per-user write backpressure. Snippet sync must reuse this
  orchestration pattern, not add a second cloud-owned React list.
- `dictionarySync.ts` additionally (post-011 review hardening, 2026-07-16):
  quarantines remote rows the local contract can never accept (skips them,
  never aborts the whole sync, never deletes them), and upserts **only changed
  records** by diffing reconciled winners against the snapshot's
  `deterministicIdentities` — otherwise every mutation rewrites the entire set.
  `upsertCloudDictionaryRecords` early-returns on an empty record list. Snippet
  sync must replicate all three behaviors from day one.
- Dictionary encryption helpers remain private to `dictionary/mod.rs`. Copying
  them again would create two implementations of key/envelope behavior.

### Frontend and rules

- `src/prototype/constants.ts::NAV_ITEMS_TOP` contains Home, Models, Dictionary,
  History, Stats, Pro, and Pro Tools. Add Snippets immediately after Dictionary.
- `src/App.tsx` already contains the Quiet Ink dictionary page, local-first
  mutation helpers, authentication reconciliation, retry UI, and search/list
  patterns to mirror without coupling the two feature states.
- `docs/firebase/firestore.rules` validates `dictionaryTerms` with an owner-only
  nested collection. The CI guard `scripts/security/check-firestore-rules.ps1`
  currently checks only the existing schemas and must learn the snippet schema.

## Target architecture

### Stored domain model

Use a separate `snippet` module with these semantic shapes (field names may be
idiomatic Rust internally; IPC remains camelCase):

```rust
pub struct VoiceSnippet {
    pub snippet_id: String,          // device-local UI identity; never synced
    pub trigger: String,             // retained display form
    pub normalized_trigger: String,  // authoritative sync/match identity
    pub expansion: String,
    pub created_at_utc_ms: u64,
    pub updated_at_utc_ms: u64,
}

struct StoredSnippetRecord {
    snippet_id: String,
    trigger: String,
    normalized_trigger: String,
    expansion: String,               // empty when deleted
    created_at_utc_ms: u64,
    updated_at_utc_ms: u64,
    deleted_at_utc_ms: Option<u64>,
}

pub struct VoiceSnippetSyncRecord {
    pub trigger: String,
    pub normalized_trigger: String,
    pub expansion: String,
    pub created_at_utc_ms: u64,
    pub updated_at_utc_ms: u64,
    pub deleted_at_utc_ms: Option<u64>,
}
```

The public list returns active `VoiceSnippet` rows only. Sync records include
tombstones and exclude `snippet_id`. Use the same monotonic local timestamp and
deterministic last-write-wins/tombstone tie policy as Plan 011. On exact timestamp
ties, compare a canonical serialized content tuple so every device selects the
same winner without depending on input order.

### Protected expansion lifecycle

Implement a small `SnippetExpansionPlan` value owned by one dictation:

```text
sanitized spoken text
  → match active triggers and create opaque per-dictation slots
  → deterministic formatting operates on protected text
  → optional LLM sees protected text only
  → validate that every expected slot appears exactly once and no unknown slot appears
  → restore exact expansions immediately before insertion/event/history
```

Each slot must include a high-entropy per-dictation nonce and index, be
unforgeable by normal speech, and never be serialized. `SnippetExpansionPlan`
owns the slot-to-expansion mapping and exposes only:

- `protect(input, snippets) -> ProtectionOutcome` (`NoMatch`, `ExactOnly`, or
  `Inline { protected_text, ... }`)
- `validate_candidate(candidate) -> Result<(), ProtectionError>`
- `restore(candidate) -> Result<String, ProtectionError>`
- `has_matches()` / `source_text()` for state-level policy

For inline matches, preserve an untouched protected baseline. If deterministic
formatting corrupts a slot, fall back to that baseline. If polish corrupts a
slot, reject it and use the valid deterministic candidate. Never attempt fuzzy
repair of a changed token. For v1, skip `apply_code_mode` for a dictation that
contains a snippet match; exact snippet integrity takes precedence over an
identifier-style transform whose whole-input behavior cannot preserve opaque
spans. Other deterministic formatting may run only while slot validation passes.

## Commands you will need

Run from `C:\Users\posan\OneDrive\Desktop\voice vibe` unless noted.

| Purpose | Command | Expected on success |
|---|---|---|
| Focused Rust storage tests | `cargo test --manifest-path src-tauri/Cargo.toml snippet::` | exit 0; new manager, migration, reconciliation, and protection tests actually run |
| Runtime pipeline tests | `cargo test --manifest-path src-tauri/Cargo.toml snippet_` | exit 0; transcript/state snippet tests actually run (do not accept a zero-test filter) |
| Tauri compile | `cargo test --manifest-path src-tauri/Cargo.toml --no-run` | exit 0 |
| Frontend focused tests | `npx vitest run src/lib/snippetSync.test.ts src/lib/cloudSync.test.ts src/lib/tauri.test.ts src/App.test.tsx` | exit 0 |
| Firestore contract | `npm run security:firestore-rules -- -Enforce` | `Firestore rules checks passed.` |
| Frontend build | `npm run build` | exit 0 |
| Patch hygiene | `git diff --check -- src-tauri/src/secure_store.rs src-tauri/src/snippet/mod.rs src-tauri/src/transcript/mod.rs src-tauri/src/state.rs src-tauri/src/lib.rs src/prototype/constants.ts src/types/voicewave.ts src/lib/tauri.ts src/lib/cloudSync.ts src/lib/snippetSync.ts src/hooks/useVoiceWave.ts src/App.tsx docs/firebase/firestore.rules scripts/security/check-firestore-rules.ps1` | no output, exit 0 |

## Scope

**In scope** (only these product files plus their focused tests):

- `src-tauri/src/secure_store.rs` (NEW: shared encrypted-envelope primitive)
- `src-tauri/src/snippet/mod.rs` (NEW: model, encrypted manager, matcher/protection)
- `src-tauri/src/dictionary/mod.rs` (only migrate encryption calls to the shared primitive; preserve file compatibility)
- `src-tauri/src/transcript/mod.rs`
- `src-tauri/src/state.rs`
- `src-tauri/src/lib.rs`
- `src/types/voicewave.ts`
- `src/lib/tauri.ts`, `src/lib/tauri.test.ts`
- `src/lib/cloudSync.ts`, `src/lib/cloudSync.test.ts`
- `src/lib/snippetSync.ts`, `src/lib/snippetSync.test.ts` (NEW)
- `src/hooks/useVoiceWave.ts`
- `src/prototype/constants.ts`
- `src/App.tsx`, `src/App.test.tsx`
- `docs/firebase/firestore.rules`
- `scripts/security/check-firestore-rules.ps1`
- `plans/README.md` (status only after implementation)

**Out of scope**:

- Team/shared snippets, organization libraries, roles, or permissions.
- Dynamic variables (`{date}`, clipboard, cursor placement), nested/recursive
  expansion, regex triggers, or app-specific snippets.
- Snippet import/export, bulk delete, folders/tags, analytics, or default snippet
  packs. These are follow-ups after v1 behavior is stable.
- Feeding expansions into ASR terminology. Triggers may be included in the ASR
  hint only if a later measured experiment proves recall improves without
  displacing dictionary terms.
- Mobile/web surfaces, billing activation, or a new entitlement tier.
- A broad `state.rs`/`App.tsx` refactor. Keep the new domain cohesive, but do not
  turn this feature into unrelated god-file cleanup.
- Any plaintext logging, telemetry, or diagnostics containing trigger/expansion
  content.

## Git workflow

- Suggested branch: `codex/012-protected-voice-snippets`
- Commit by coherent vertical slice; observed style is Conventional Commits,
  e.g. `feat(snippets): add protected local-first expansion`.
- Do not push or open a PR unless the operator explicitly asks.
- Never stage unrelated existing working-tree changes.

## Steps

### Step 1: Extract the reusable encrypted-envelope primitive

Create private `src-tauri/src/secure_store.rs` and register it from `lib.rs`.
Move only the generic AES-256-GCM envelope, key-file loading/creation, and JSON
serialize/encrypt/decrypt mechanics out of `dictionary/mod.rs`. Preserve envelope
version `1`, nonce/ciphertext base64 encoding, field names, key size, and atomic
write behavior exactly. The helper must accept a domain-specific label so errors
still say dictionary or snippets without exposing content.

Switch `DictionaryManager` to this helper without changing dictionary paths,
key paths, stored schema, public errors, or migration behavior. Add a compatibility
test that loads an envelope produced by the pre-extraction shape, plus the existing
encrypted-at-rest assertion. This is a refactor gate: no snippet code proceeds
until the dictionary tests prove old encrypted data still opens.

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml dictionary::` → exit
0; compatibility and existing dictionary tests run and pass.

### Step 2: Implement the encrypted local snippet manager

Create `src-tauri/src/snippet/mod.rs` with the model above. Use
`snippets.json` and `snippets.key` in the same VoiceWave config directory, but
through `secure_store`; never reuse the dictionary key. The manager owns all
normalization, validation, identity, timestamps, persistence, and reconciliation.

Required operations:

- `list_snippets(query)` searches active trigger and expansion text locally,
  case-insensitively, and returns a stable trigger sort.
- `add_snippet(trigger, expansion)` rejects an active normalized duplicate.
- `update_snippet(snippet_id, trigger, expansion)` updates content in place when
  identity is unchanged. On rename, atomically tombstone the old identity and
  create/resurrect the new identity; reject collision with another active row.
- `remove_snippet(snippet_id)` tombstones it and clears expansion content.
- `get_sync_records()` returns active rows and tombstones without local IDs.
- `reconcile_records(remote)` validates every remote row, merges deterministically,
  persists once, and returns both active local rows and canonical sync winners.

Use a temp file plus rename for persistence. On a failed write, leave the
in-memory store unchanged or restore its previous snapshot so local state never
claims a mutation that was not durable. Enforce the 250-active cap after merge;
if remote reconciliation would exceed it, return a typed non-retryable limit
error without partially applying records.

Tests must cover encryption/no plaintext, reload, normalization, duplicate,
same-trigger edit, atomic rename, rename collision, delete content clearing,
re-add, clock rollback, timestamp ties in both input orders, remote tombstone vs
local active, local tombstone vs remote stale active, limit behavior, malformed
remote rows, and failed-persist rollback.

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml snippet::manager` →
exit 0; named manager cases run.

### Step 3: Build Unicode-aware protected matching

In the snippet module, implement matching and `SnippetExpansionPlan`; keep it
independent of Tauri and the UI. Normalize trigger identity with the same
NFC/case/whitespace function used by the manager. Match against a normalized
view of the spoken source while retaining byte-span mapping to the original so
unmatched text is not reconstructed or case-folded.

Boundary semantics: a match is invalid when the character immediately before or
after it is a Unicode letter, number, or combining mark. Punctuation and
whitespace are valid boundaries. Sort candidates by normalized scalar length
descending and then stable normalized identity; select non-overlapping spans
left-to-right. Replace all selected spans in one pass so expansion output is
never recursively scanned as another trigger.

Exact-only recognition strips surrounding whitespace and at most one terminal
`.`, `!`, or `?` added by ASR before comparing. It returns the expansion directly,
not a tokenized transcript. Inline protection uses per-dictation nonce slots,
validates exact slot multiplicity, and refuses more than 16 occurrences.

Tests must include ASCII case-insensitivity, NFC composed/decomposed identity,
non-Latin scripts, combining marks, whitespace collapse, punctuation adjacency,
no substring match, longest overlap, repeated and multiple triggers, no recursive
expansion, exact-only terminal punctuation, literal multiline restoration,
token deletion/duplication/mutation/invention, and the 16-occurrence limit.

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml snippet::expansion` →
exit 0; named matching/protection cases run.

### Step 4: Integrate protection at the dictation orchestration boundary

Add `snippet_manager: Arc<Mutex<SnippetManager>>` to `VoiceWaveController` and
construct it beside the dictionary manager. Snapshot active snippets once per
dictation, after sanitized speech exists and before deterministic finalization.
Do not hold its mutex during formatting, inference, insertion, or cloud work.

Keep snippet orchestration in `state.rs`, because exact-only short-circuiting and
all polish paths converge there:

1. Create the protection outcome from the sanitized spoken transcript.
2. `NoMatch`: preserve the current pipeline byte-for-byte.
3. `ExactOnly`: use the exact expansion as the delivery text, set a snippet
   outcome, and skip deterministic formatting, blocking polish, background
   polish, late polish, and correction-candidate learning.
4. `Inline`: pass `protected_text` through the selected deterministic path. Tell
   `finalize_pro_transcript` to skip `apply_code_mode` for this dictation. Validate
   slots after finalization; if invalid, use the untouched protected baseline.
5. Send only valid protected text to any enabled LLM polish path. After the
   existing fidelity gate accepts a candidate, require snippet slot validation.
   Invalid candidates follow the existing rejected/fallback outcome.
6. Restore expansion text exactly once, immediately before setting/emitting
   `inserted_text`. Feed restored text to insertion, snapshot, History, and a
   user-visible polish Copy offer. Feed spoken pre-expansion text to integrity
   checks. Do not derive correction candidates for either side of a correction
   session when that dictation contains a snippet.
7. **Multiline delivery safety.** If the restored delivery text contains
   newline characters, prefer the clipboard insertion method over
   character-wise direct input for that dictation. A typed newline is an Enter
   keystroke, and in chat/form inputs it submits the message mid-expansion;
   pasted newlines are treated as soft line breaks by those same apps. Reuse
   the existing method-selection machinery (this is the same class of override
   as terminal detection); keep the normal chain for single-line expansions.

Audit every `final_transcript`/`inserted_text` consumer in the immediate,
wait-validated, async background, timeout/late-result, insertion rescue, history
update, and pill event branches. Add an internal assertion/helper so no branch
can emit a value containing the slot prefix.

Do not add expansions to `ProTranscriptOptions`; that recreates the old unsafe
design. The transcript module only receives a boolean/policy needed to skip
code mode and otherwise processes protected text like ordinary input.

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml snippet_` → exit 0;
tests prove no-match equivalence, exact-only bypass, inline deterministic restore,
polish accept/reject, async/late offer restore, history restore, and correction
learning exclusion.

### Step 5: Expose cohesive Tauri CRUD and reconciliation commands

In `state.rs` add controller methods and in `lib.rs` add/register commands with
camelCase IPC types:

- `list_voice_snippets(query?)`
- `add_voice_snippet(trigger, expansion)`
- `update_voice_snippet(snippetId, trigger, expansion)`
- `remove_voice_snippet(snippetId)`
- `get_voice_snippet_sync_records()`
- `reconcile_voice_snippet_records(records)`

Return structured, stable error codes (validation, duplicate, not-found, limit,
identity-mismatch, persistence) rather than making the frontend parse prose.
CRUD commands return the new active list or enough data for the hook to refresh
from Rust. Do not put cloud calls inside Tauri commands.

Add TypeScript models and bridges in `src/types/voicewave.ts` and
`src/lib/tauri.ts`. Test exact invoke command/argument names, particularly
`snippetId`, and malformed response handling if the existing bridge validates it.

**Verify**: `npx vitest run src/lib/tauri.test.ts` and
`cargo test --manifest-path src-tauri/Cargo.toml snippet_` → both exit 0.

### Step 6: Add deterministic owner-only cloud replication

Create `src/lib/snippetSync.ts` following `dictionarySync.ts`:

```text
read remote records
  → Rust reconcile (the only winner policy)
  → upsert canonical winners
  → return Rust's active local list
```

Extend `cloudSync.ts` with a separate `voiceSnippets` collection. Document ID is
`snippet-${encodeURIComponent(normalizedTrigger)}`. Before writing, TypeScript
recomputes normalized identity and rejects disagreement with Rust; never trust a
path-like or mismatched remote identifier. Reuse the 500-write batch limit,
content hashing, per-user backpressure, retry classification, and guardrail event
shape. Do not log trigger or expansion text.

Firestore documents contain only:

```text
trigger, normalizedTrigger, expansion,
createdAtUtcMs, updatedAtUtcMs, deletedAtUtcMs
```

Active rows require non-empty expansion within 4,000 scalars. Tombstones require
`expansion == ""`. Rules allow only the authenticated owner, exact keys, bounded
strings, reasonable timestamps, `updated >= created`, and `deleted >= updated`.
Update `check-firestore-rules.ps1` so CI fails if the snippet validator or nested
collection policy disappears.

No legacy snippet migration exists. Never delete arbitrary remote documents on
a read. Mirror the hardened `dictionarySync.ts` behaviors exactly:

1. **Quarantine, don't abort.** A remote row whose trigger or expansion can
   never pass local validation (empty, control characters, overlength) is
   skipped client-side — excluded from the records sent to Rust and never
   deleted — so one poisoned document cannot permanently brick every sync.
   Rust stays fail-closed on identity mismatch for rows that do arrive.
2. **Upsert only changed records.** Return `deterministicIdentities` from the
   snapshot and diff reconciled winners against the fetched records; skip
   rewriting documents that already match. Without this, every snippet edit
   costs N document writes.
3. **Early-return on an empty upsert list** before backpressure accounting.

Tests cover deterministic IDs, owner paths, field mapping, tombstones,
identity mismatch, batching, retry/backpressure, quarantined poison rows,
unchanged-record skip, and no content in guardrail diagnostics.

**Verify**: `npx vitest run src/lib/snippetSync.test.ts src/lib/cloudSync.test.ts`
and `npm run security:firestore-rules -- -Enforce` → all exit 0.

### Step 7: Add hook state and auth-ready local-first orchestration

In `useVoiceWave.ts`, keep `voiceSnippets` sourced only from Rust. Expose refresh,
add, update, remove, and `syncVoiceSnippetsWithCloud(uid)`. Every mutation awaits
the local Tauri operation first, immediately refreshes local state, then lets
`App.tsx` attempt cloud reconciliation. A cloud failure must leave the local
result visible and set sync status to pending.

In `App.tsx`, reconcile snippets at the same auth-ready points as dictionary
sync: startup/user restoration, sign-in, retry, and post-local mutation. Sign-out
changes the label to device-local but retains the same encrypted local data.
Keep dictionary and snippet statuses independent so one failing collection does
not falsely mark the other synced.

Prevent stale async completion from a prior user session overwriting current
status: capture the UID/session generation and ignore results after sign-out or
account change.

When reconciliation fails with the typed 250-active-limit error, surface an
actionable message ("Snippet limit exceeded across your devices — delete some
snippets to resume sync"), not a generic retry: retrying cannot succeed until
the user deletes rows, so a bare Retry button would be dishonest UI.

**Verify**: add focused hook/App tests for signed-out local add, signed-in local
add with failed cloud sync, auth-ready reconciliation, manual retry, sign-out
retention, and stale completion. Run `npx vitest run src/App.test.tsx` → exit 0.

### Step 8: Build a first-class Quiet Ink Snippets page

Add `{ id: "snippets", label: "Snippets", ... }` immediately after Dictionary
in `NAV_ITEMS_TOP`, with a distinct Lucide icon. In `App.tsx`, add a Snippets page
that uses existing Quiet Ink surfaces and button/input classes:

- Header with active count and honest sync label (`On this device`, `Syncing`,
  `Synced`, `Changes pending`) plus Retry only when useful.
- Search across trigger and expansion; `Ctrl/Cmd+F` focuses search while on the
  page and `Ctrl/Cmd+N` opens the form. Do not steal shortcuts outside the page.
- Add/edit form with separate spoken-trigger input and multiline exact-expansion
  textarea, live limits, validation, short/common-trigger warning, Save/Cancel,
  and disabled state during the local mutation only.
- Rows show trigger prominently and a whitespace-preserving, safely truncated
  expansion preview. Edit reuses the form. Delete requires confirmation because
  it propagates across devices.
- Empty state teaches one concrete example and explains that expansion text is
  inserted exactly as saved. Never imply that snippets improve recognition.
- Accessible labels, keyboard focus restoration, Escape-to-cancel, and a live
  status region for save/delete/sync outcomes.

Do not reuse the dictionary pending-suggestion UI or merge the pages. Do not use
`dangerouslySetInnerHTML`; render expansion as plain text.

Tests cover nav, empty state, search, add validation, multiline expansion, edit
with trigger rename, delete confirmation, retry state, keyboard shortcuts scoped
to the page, and accessible names.

**Verify**: `npx vitest run src/App.test.tsx` then `npm run build` → exit 0.

### Step 9: Run the focused release gate and perform one desktop smoke test

Run every command in "Commands you will need." Then launch the desktop app and
manually verify with a temporary snippet whose expansion contains mixed case, a
URL, punctuation, and a newline:

1. Exact-only trigger inserts byte-for-byte without an added period.
2. Inline trigger expands once while surrounding dictated prose is formatted.
3. The behavior remains exact with each available polish profile enabled.
4. Repeated trigger expands repeatedly; a trigger inside a larger word does not.
5. Restart offline and confirm the snippet persists and works.
6. Sign in, edit the expansion, reconcile, and confirm another device/session
   receives the edit; delete and confirm it does not resurrect.
7. Inspect History and any polish Copy offer: restored text is visible and no
   internal slot appears.

Remove the temporary snippet through the UI. Do not inspect or print plaintext
store/key contents during the smoke test.

**Verify**: record the commands and smoke-test outcomes in the implementation
handoff; all automated commands exit 0 and all seven checks pass.

## Test plan

The step-level tests are required. The minimum regression matrix is:

- **Storage/security**: encrypted-at-rest, old dictionary envelope compatibility,
  separate keys, atomic failed-write rollback, no deleted expansion retention.
- **Identity/sync**: Unicode normalization, deterministic document identity,
  add/edit/atomic rename/delete/re-add, LWW ties independent of input order,
  offline mutation, retry, stale auth completion, no tombstone resurrection.
- **Matching**: exact-only, inline, multiple/repeated, overlap, Unicode boundaries,
  punctuation, no substring, no recursion, occurrence cap.
- **Protection**: exact multiline restoration, expansion absent from LLM input,
  token corruption rejected, deterministic corruption fallback, no token in
  insertion/event/history/pill, code-mode bypass only when matched.
- **Quality isolation**: no-match output matches the pre-feature result;
  pre-expansion ASR integrity; correction learning excluded for snippet-involved
  dictations.
- **UI**: searchable CRUD, edit rename, confirmation, sync status/retry, limits,
  scoped keyboard actions, accessible names.

Do not inflate the suite with snapshot churn. Prefer pure manager/matcher tests,
the existing cloud-sync mocks, exact Tauri bridge assertions, and a few behavior
tests in `App.test.tsx`.

## Done criteria

- [ ] Exact-only and inline voice snippets work with byte-for-byte saved expansions.
- [ ] Expansion content never enters deterministic transforms or any LLM request.
- [ ] Every polish path validates placeholders and falls back safely.
- [ ] No placeholder can reach insertion, events, History, correction learning, UI, or logs.
- [ ] CRUD is encrypted, atomic, local-first, offline-capable, and searchable.
- [ ] Trigger rename and delete reconcile across devices without stale resurrection.
- [ ] Snippets have a separate first-class navigation page and honest sync state.
- [ ] No-match dictation behavior is unchanged by characterization tests.
- [ ] All commands in "Commands you will need" exit 0 and the desktop smoke test passes.
- [ ] `git diff --check` reports no patch hygiene errors.
- [ ] No unrelated existing working-tree changes are staged, reverted, or overwritten.
- [ ] `plans/README.md` marks Plan 012 DONE only after every criterion passes.

## STOP conditions

Stop and report, with the exact failing command or mismatch, if:

- Plan 011's local-first dictionary reconciliation is absent or no longer the
  architecture described above.
- Extracting `secure_store` changes the dictionary envelope, key path, or makes
  an existing encrypted dictionary unreadable.
- A deterministic transcript path cannot preserve every expected slot and the
  protected-baseline fallback also fails validation.
- Any LLM/polish request contains a saved expansion in a test or instrumented
  local run.
- Correct Unicode matching would require silently replacing the user's original
  unmatched text with a normalized reconstruction.
- Reconciliation cannot make trigger rename/delete atomic without partially
  committing local state.
- Firestore rules cannot validate active versus tombstoned expansion shape with
  the deployed rules version.
- A required change expands into billing, team sharing, mobile, or a broad
  transcript/state refactor.
- A verification command fails twice after a reasonable scoped fix.

## Maintenance notes

- The protection abstraction is the future seam for cursor placeholders and
  dynamic fields. Do not implement those by performing recursive string replace.
- **Known v1 matching limitation (deliberate):** Whisper may insert punctuation
  inside a multi-word trigger ("my support, reply"), which defeats both
  exact-only and inline matching because normalization only folds case,
  whitespace, and NFC. Ship v1 as specified; if real-world recall proves poor,
  the correct fix is a punctuation-insensitive normalized view with span
  mapping (same architecture, wider fold) — never fuzzy matching. UI copy can
  nudge users toward distinctive multi-word triggers, which also reduces
  accidental expansion.
- If code-mode snippets become important, add a protected-span-aware code
  formatter with dedicated tests; do not remove the v1 bypass casually.
- Tombstone compaction needs a server-observed convergence/retention design.
  Keeping small content-free tombstones is safer than time-based local deletion.
- Team/shared snippets require a separate ownership and conflict model. Do not
  overload the personal `users/{uid}/voiceSnippets` collection.
- Reviewer focus: prove expansion text is absent at the LLM boundary; enumerate
  every async polish branch; test rename/delete conflict convergence; inspect
  Unicode span mapping and the deterministic tie-breaker.
- Plan 004 remains historical evidence only and must not be partially combined
  with this plan.
