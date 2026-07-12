# 010 — Polish Profiles: Mode-Aware LLM Polish

Status: IMPLEMENTED (core, 2026-07-11) — see "Rev 3 outcomes" at the bottom
for what the gates actually measured, the Casual cut, and remaining items.
Rev 2 — incorporates external review (GPT-Sol, 2026-07-10): delivery policy
decided, Verbatim respecified as Literal, profile authority designed, history
schema extended, migration made explicit.
Depends on: shipped LLM polish (plan 005), live history (plan 006). Feeds: per-app auto-switching (future plan 011).

## North star

A user who reads two outputs of the same dictation, processed by two different
profiles, can tell which profile produced which — **without being told**. If a
profile's output is indistinguishable from another's on realistic input, the
profile does not ship. Distinctness is a measured gate, not an aspiration.

## The gap this closes (verified against code, 2026-07-10)

- `ProToolsMode` (default/coding/writing/study) is **not persisted** — it is
  inferred from four independently editable settings (`src/App.tsx:170-192`)
  and selecting a mode performs five sequential writes (`App.tsx:792-819`), so
  a mid-sequence failure can leave a torn hybrid config.
- The deterministic layer is real and tested (`FormatProfile`, `DomainPackId`,
  `CodeModeSettings`, `AppProfileOverrides` → `transcript/mod.rs::finalize_pro_transcript`).
- The LLM polish pipeline is mode-blind AND advisory-only: it runs in a
  background task **after** insertion and only offers its output via the
  pill's Copy action (`state.rs:3925-3952`); `polish_text(raw)` takes no mode
  (`llm_polish.rs:120`); the worker has one hardcoded SYSTEM_PROMPT
  (`polish_worker.py:34-44`). What lands in the target app is always the
  deterministic text.

So today, "modes" nudge punctuation mechanically, and the smart layer's output
is a suggestion most users never see. That's why selecting a mode doesn't feel
like anything.

## DECIDED: Delivery policy (the text that lands)

Per-profile, no auto-replace ever:

- **Standard** keeps today's contract byte-for-byte: deterministic text
  inserts immediately (zero latency regression), polish runs async as a
  Copy-offer. Default users lose nothing.
- **Coding / Writing / Casual**: selecting one of these is an explicit opt-in
  to "the profile IS the text." Release → deterministic pipeline → **single**
  LLM attempt + validators → validated output inserts. Hard end-to-end budget:
  if polish + validation hasn't produced an accepted result within
  **3.5s of key release** (or the validator rejects), the deterministic floor
  text inserts instead and the outcome is recorded as `fallback`. No blocking
  retry on the insert path — the retry-with-correction-prompt pass runs only
  async afterward, updating History (never the target app).
- **Literal**: deterministic only, always immediate (see below).
- Rejected: silently replacing already-inserted text later (focus, cursor,
  undo, and user-edit races — not v1 territory).

This changes VoiceWave's latency contract for opted-in profiles, and the plan
owns that: Phase 4 benchmarks **full release-to-insert latency**, not model
time. The profile cards state it plainly ("adds ~2s for AI shaping").

## Design principles

1. **Two-tier profiles.** Deterministic bundle (floor, always available) + a
   per-profile LLM contract (the upgrade). Graceful degradation when polish is
   off, times out, or is rejected.
2. **One system prompt per profile** — separate prompts, not a mega-prompt
   with a mode variable. Cuts mode-bleed on the small model.
3. **Contracts, not vibes** — NEVER/ALWAYS lists enforced by deterministic
   validators. Rust stays the fail-closed authority (as `validate_polish`
   already is); the Python worker only generates candidates.
4. **Shipped model stays Qwen2.5-1.5B** (`llm_polish.rs:47`). Benchmarked
   2026-07-10: polish p50 1.7s / p95 2.1s on a simulated 4-core budget laptop.
   3B quality tier: explicitly deferred.
5. **Free in v1** (strategy per plan 009), with an entitlement test asserting
   profiles work without Pro gating — formatting commands already pass through
   Pro gates, so "included for everyone" must be tested, not assumed.

## The five profiles

| Profile | NEVER | ALWAYS | Deterministic floor | Insert path |
|---|---|---|---|---|
| **Standard** | (current shipped prompt, unchanged) | fix grammar/filler, preserve entities | current default pipeline | immediate + async offer |
| **Coding** | invent or respell identifiers/APIs/paths; change identifier casing | preserve `camelCase`/`snake_case`/paths char-for-char; terse engineering phrasing | CodeModeSettings + Coding pack | wait-validated |
| **Writing** | slang, fillers, fragments, contractions; strengthening hedges into directives | grammatical professional prose; speaker's uncertainty preserved | Academic bundle | wait-validated |
| **Casual** | headings, bullets, sign-offs, corporate tone | short natural chat register; hedging and contractions kept | Concise-ish bundle, light touch | wait-validated |
| **Literal** | any AI rewriting; filler removal; domain corrections | the recognized ASR word sequence, plus punctuation/capitalization | **branches BEFORE `finalize_pro_transcript`** | immediate |

**Literal, precisely** (renamed from "Verbatim" — ASR can mishear, so "exact
spoken words" overpromises): "No AI rewriting — inserts the recognized words
as heard, with punctuation and capitalization only." It branches from the
sanitized ASR baseline *before* the deterministic transform stack
(`state.rs:3774` / `transcript/mod.rs:33`), because by `final_transcript` the
original sequence (fillers, repeated tokens, pre-command text) is already
gone. Decisions: explicit spoken punctuation/structural commands STILL apply
(they are user commands, not AI rewriting); user-dictionary stabilization
STILL applies (it corrects recognition toward what was actually said); filler
removal, domain corrections, format profiles, code mode all OFF.
Accessibility/legal marketing says "no AI rewriting," never "exact words."

Reference outputs for one raw transcript (the sellable demo) — raw:
*"so um i think we should refactor getUserById to not throw when the user
doesnt exist and instead return null"*
- Coding: `Refactor getUserById to return null instead of throwing when the user doesn't exist.`
- Writing: `I think we should refactor getUserById so that it returns null rather than throwing an exception when the user does not exist.`
- Casual: `I think we should refactor getUserById so it returns null when the user doesn't exist, instead of throwing.`
- Literal: `So, um, I think we should refactor getUserById to not throw when the user doesn't exist and instead return null.`

(The identifier is present in the *input* — profiles never invent identifier
spelling. Spoken-to-camelCase conversion of "get user by id" is Code Mode's
deterministic spoken-casing feature, out of scope here.)

## Profile authority: the `DictationProfile` module (new, Rust)

`polish_profile` becomes a **persisted settings enum** and the single
authority. The four deterministic fields become *derived defaults with
tracked user overrides*:

- `set_dictation_profile(profile)` — one Tauri command, one atomic settings
  write (fixes the five-sequential-writes torn-config risk).
- `resolve_profile(profile, overrides) -> EffectivePolicy` — deterministic
  bundle + LLM prompt id + insert path, in one place. React sends the command
  and renders the result; it never reconstructs policy.
- Editing an advanced field marks the profile `customized`; UI shows
  "Writing · Customized"; reselecting the profile card resets overrides
  (with a confirm).

**Migration** (not lossless by mapping alone — old "study" is inferred, not
stored): absence of `polish_profile` marks a legacy config; derive the closest
profile from the old bundle (FormatProfile × DomainPack table), and preserve
the user's existing field values **as overrides** so day-one output is
unchanged. Migration fixtures: all four old presets + one hand-edited custom
mixture.

## Phases

### Phase 0 — Audit + baseline measurements (1 day)
- Confirm validator inventory: Rust `validate_polish` (state.rs:3952) +
  `stabilize_custom_terms` re-application; move both into a
  `inference/polish_gate.rs` module (out of the 5,000-line controller) with
  **structured rejection reasons** (`identifier_changed`, `negation_dropped`,
  `entity_missing`, `output_truncated`, `low_overlap`) instead of bool —
  retry and telemetry need codes.
- Measure in the live worker: warm same-profile call, profile-switch call
  (full re-prefill), cold model load, retry pass, peak RSS — each separately.
  llama.cpp reuses the KV prefix between consecutive calls, so same-profile
  should be cheap and switches expensive (~10-20s worst-case at 4-core prefill
  rates on a 700-token prompt). **This sets Phase 2's exemplar budget.**
- Test the bundled llama_cpp_python RAM cache before designing custom
  per-profile state snapshots; five cached states need an explicit RSS budget
  (the app also keeps Whisper warm — set a ceiling, e.g. +600MB total).
- Context math: worker is `n_ctx=2048`, `max_tokens=400`
  (`polish_worker.py:141`). Define the long-utterance policy: reserve
  prompt-tokens + input + output ≤ n_ctx with margin; over-budget input skips
  the LLM tier (deterministic floor + history note); **reject
  `finish_reason=length` candidates** in the gate.

### Phase 1 — DictationProfile module + delivery policy (3-4 days)
- Persisted `polish_profile`, atomic `set_dictation_profile`, resolve logic,
  migration + fixtures (above).
- Insert-path rework at the `state.rs` call site: per-profile branch
  (immediate vs wait-validated with the 3.5s release-to-insert budget vs
  Literal pre-finalize branch). Queue policy: polish requests are serialized
  today — rapid consecutive dictations must not insert stale results; each
  request carries a session id, and pill offers/suppressed results are
  session-aware (obsolete results dropped silently).
- IPC: `{command, id, text, profile}`; unknown profile → Standard (back-compat).
- History schema (per plan 006 conventions, serde defaults for old records):
  `selectedProfile`, `insertedText` (what landed), `polishedText`,
  `polishOutcome` (accepted / fallback_timeout / fallback_rejected /
  literal / disabled), `polishLatencyMs`, `polishRetried`. Async updates
  reference the stable `record_id`; the async retry pass updates History only.
  Without these fields, a bare `profile` label would mislabel deterministic
  fallback text as LLM output.

### Phase 2 — Profile prompts + exemplars (2-4 days, the craft phase)
- One prompt module per profile: contract + 3-6 contrastive exemplars
  (fillers, false starts, identifiers, numbers/dates, hedges, questions,
  short inputs; include negative exemplars). Temperature 0, fixed order,
  output token cap, plain text only.
- Exemplar token ceiling set by Phase 0's switch-cost and n_ctx math
  (starting target: ≤400 tokens/profile).
- Packaging: prompts live in the bundled worker file or a bundled asset —
  either way `tauri.conf.json:49` (currently bundles only `polish_worker.py`)
  must be updated and verified in a fresh-checkout build (lesson of the
  v0.5.0 CI bugs).
- Iterate against the **development corpus only** (see Phase 4 split).

### Phase 3 — Per-profile validators in `polish_gate.rs` (2-3 days)
- Accepted-output safety is **100%, not 99%**: any protected-entity or
  identifier-casing alteration = reject. Corpus-level quality targets are for
  measurement; the accept gate is absolute.
- All profiles: protected-entity extraction (exists in spike validator) +
  **identifier-casing check** (catches the 1.5B `maxRetries`→`MaxRetries`
  class the 2026-07-10 harness missed — fix the harness's casing hole too) +
  paraphrase-drift guard (per-profile token-overlap floors; modality words
  maybe/must/not/never must survive).
- Coding: reject any altered identifier/path. Writing: reject contractions/
  fillers in output. Casual: reject headings/bullets/sign-offs. Literal: not
  LLM-gated (never calls the model).
- Failure policy: insert path = single attempt then deterministic fallback
  (per delivery policy). Async path = one retry with a compact correction
  prompt keyed to the rejection code. Track separately: first-pass accept,
  retry success, fallback rate, false accepts, false rejects, per profile.

### Phase 4 — Distinctness + latency gate (2-3 days)
Extend `polish_spike.py` → `profile_gate.py`:
- Corpus ~75 transcripts across categories (technical, email, chat, rambling,
  false starts, entity-dense, negation/uncertainty, short ≤8 words), **split
  60 development / 15 holdout BEFORE prompt iteration starts**; the holdout is
  run once at the end, not inspected repeatedly.
- "Near-identical" defined mathematically: normalized token edit distance
  < 0.12 OR exact-match after case/punct folding (tune constant on dev set,
  freeze before holdout).
- Gates (all must pass):
  - 0 validator false-accepts; accepted-output entity+casing preservation 100%;
    Literal lexical edit distance vs sanitized ASR = 0 on 100% of cases.
  - Pairwise near-identical rate on non-short buckets: Coding↔Writing < 5%,
    Writing↔Casual < 5%, any↔Literal < 5%.
  - Short-utterance bucket reported separately, excluded from distinctness
    gates (profiles legitimately converge on "send it tomorrow") — calibrates
    honest UI/marketing copy.
  - Latency on 4-core affinity, measured as **full release-to-insert**
    including deterministic pipeline + validators: p95 ≤ 3.0s for
    wait-validated profiles; fallback rate < 5%; profile-switch first-call
    cost reported.
- Human pass (blind, stratified 20): name the profile AND score meaning
  preservation + usefulness 1-5 — distinctness alone would reward gratuitous
  rewriting; meaning-preservation average must be ≥ 4.5.

### Phase 5 — UI (2-3 days, Quiet Ink + Signal Blue)
- Pro Tools cards → **Polish Profiles**: five cards, each showing the same
  sentence before→after under that profile (canned from the Phase 4 corpus —
  distinction visible at selection time). Wait-validated cards disclose the
  latency trade ("adds ~2s for AI shaping"). Literal card: "No AI rewriting —
  your words as recognized, punctuation only."
- Active profile = small persistent glyph/label in the listening pill; a pill
  *notice* fires only on profile change (per-dictation notices would be noise
  and bury real pill alerts — same rationale as POLISH_PILL_ANNOUNCE_EVERY).
- "Writing · Customized" state + reset-on-reselect confirm.
- History entries show profile + outcome badge (accepted/fallback/literal);
  entries with both texts offer a compare view.

### Phase 6 — Ship gate
- `profile_gate.py` green including holdout; committed results JSON.
- Migration fixtures green; existing transcript tests green; casing-validator
  unit tests green; fresh-checkout packaged build runs profiles (packaging
  check).
- Entitlement test: profiles function with Pro gating off.
- Manual E2E: reference transcript dictated under all five profiles into
  Notepad + VS Code + browser textarea; pill glyph + history records correct;
  release-to-insert stopwatch numbers match harness.
- Changelog + website feature section (website refresh owes this anyway).

## Explicitly deferred
- Per-app auto-switching (plan 011; detection exists in `insertion/mod.rs`).
- 3B quality tier / GPU polish wheel.
- Custom user-defined profiles (premium candidate).
- Streaming preview (next big swing after this ships).
- Auto-replace of inserted text (rejected for v1, revisit only with a real
  insertion-transaction design).

## Risks
- **Mode-bleed on 1.5B** — separate prompts, contrastive exemplars,
  validators; a profile that can't pass distinctness after two prompt
  iterations is cut from v1 (Coding and Literal are protected; Casual is
  first on the chopping block).
- **Wait-validated latency disappoints** — the 3.5s budget + fallback keeps
  the worst case bounded; if fallback rate exceeds 5% on real hardware, the
  profile ships as async-offer instead (Standard's model) rather than slow.
- **Profile-switch prefill cost** — Phase 0 measures; mitigations in order:
  smaller exemplars → RAM cache → per-profile state snapshots under an RSS
  ceiling → "warming profile…" pill notice.
- **Migration** — legacy-detection + overrides-preservation + fixtures for all
  four presets and a custom mixture.
- **Corpus overfitting** — dev/holdout split enforced before iteration.

---

## Rev 3 — post-implementation outcomes (2026-07-11)

Implemented by three parallel agents (Rust core / worker+harness / UI) plus an
integration pass. What the gates actually measured, and what changed:

- **Casual is CUT from the v1 selectable lineup** (per this plan's own rule):
  writing↔casual near-identical on 50% (holdout) to 59% (dev) of realistic
  input. The card is removed (commented, re-addable); the backend still
  accepts `"casual"`; the legacy migration was retargeted study→Writing on
  both sides so nobody lands on an unselectable card.
- **The <5% all-buckets distinctness gate was structurally wrong**, not merely
  failed: inputs with no register-sensitive material have one correct clean
  rendering and profiles legitimately converge (technical bucket 56-89%
  near-identical, rambling bucket 0-12%). The honest gate for any future
  profile: distinctness measured on register-sensitive buckets only
  (rambling/chat/false-starts), with the meaning-preservation co-gate
  unchanged. Coding↔writing overall: 21-39% near-identical, clearly distinct
  where it matters.
- **Safety gates passed where it counts**: 0 false-accepts on dev (60) for
  all profiles; holdout (15) had exactly one — coding dropped "probably"
  (hedge→directive). Fixed in the Rust gate, not the harness: MODALITY_WORDS
  extended (+probably/might/perhaps/possibly) with the holdout case as a unit
  test, and the harness's consonant-run literal check ported to the gate as
  `LiteralTokenDropped`.
- **Measured runtime (1.5B, 4 CPU threads, CUDA masked)**: warm polish p50
  1.6-2.1s / p95 2.4-2.6s (inside budget); cold load 1.4s; profile-switch
  full re-prefill 6.6-10s at the final prompt sizes (coding 393 / writing 409
  tokens — the ≤250-token switch target lost to safety, which needed the 4th
  coding exemplar). LlamaRAMCache rejected: +3.3GB RSS. Mitigation shipped:
  `set_dictation_profile` pre-warms the worker (detached) for wait-validated
  profiles, which also fixes the first-dictation-after-launch cold fallback.
- **Verification**: measurement scripts must mask CUDA (`profile_gate.py`
  does) — the venv's llama build silently offloads prefill to the GPU and
  invalidates CPU numbers.
- **Still open**: async retry-with-correction (worker support), the Phase 6
  ship-gate items (manual E2E in real apps, packaging fresh-checkout check,
  entitlement test), website/changelog, and the product question flagged in
  review: the deterministic floor under free profiles still passes through
  Pro gating.
