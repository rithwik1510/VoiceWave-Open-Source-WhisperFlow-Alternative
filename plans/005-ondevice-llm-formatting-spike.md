# Plan 005: On-device LLM "polish" pass — time-boxed feasibility spike

> **Executor instructions**: This is a SPIKE, not a ship-it plan. The
> deliverable is EVIDENCE and a GO/NO-GO recommendation, plus a prototype
> behind an off-by-default flag — NOT a production feature. Follow the phases;
> at each GATE, record the measured result in `plans/artifacts/005-llm-spike.md`
> and honor the go/no-go. If a STOP condition occurs, stop and report. Do not
> turn this on by default under any circumstances.
>
> **Drift check (run first)**: written against commit `d29927d` **plus
> uncommitted working-tree changes** (do NOT `git stash`). Confirm the "Current
> state" excerpts before starting; on mismatch, STOP.

## Status

- **Priority**: P1 (flagship value) — but gated as a spike; productionization is a SEPARATE plan written only if the spike says GO
- **Effort**: L (multi-day spike)
- **Risk**: HIGH (adds a dependency + a model; the feature fights a core product invariant — the spike exists to measure that risk before committing)
- **Depends on**: none to start the spike
- **Category**: direction
- **Planned at**: commit `d29927d` + uncommitted working tree, 2026-07-04

## Why this matters

VoiceWave's post-processing is 100% deterministic string rules — it can fix
casing and format lists, but it structurally CANNOT rephrase speech into text
that "reads like writing." That rephrasing is Wispr Flow's core selling point.
An OPTIONAL on-device LLM rewrite pass would close that gap while keeping the
100%-local privacy moat (the ADR forbids CLOUD rewrite, not on-device). The
architecture makes the WIRING cheap (an async post-insert "swap" seam already
exists), but whether a small local model can rewrite real dictation WITHOUT
hallucinating or altering names/numbers is an unproven research question. This
spike answers that question with data before anyone commits to building it.

## Current state (the facts the spike relies on)

- **The insertion point is post-insert and async — NOT inside `finalize_pro_transcript`.**
  - `finalize_pro_transcript` (`src-tauri/src/transcript/mod.rs:32-60`) is a PURE,
    SYNCHRONOUS function called INLINE on the hot path at `src-tauri/src/state.rs`
    (grep `finalize_pro_transcript(` — the call sits before the transcript is
    emitted and before `insert_text`). Putting an LLM call here would add
    model-decode latency directly between speech-release and text-insertion. DO
    NOT put the LLM in this function. Keep it pure.
  - The correct seam: a NEW async stage AFTER insertion. `state.rs` already:
    - keeps a `CorrectionSession { inserted_text, inserted_at_utc_ms }` (`state.rs:933`),
      set right after a successful insert (grep `CorrectionSession {` — set near the
      insertion-success block ~`state.rs:3565`);
    - spawns non-critical work off the hot path via `tauri::async_runtime::spawn`
      (grep the comment "Keep non-critical persistence off the hot path" in `state.rs`);
    - can re-surface replacement text through the rescue pill `emit_pill_rescue`
      (grep `fn emit_pill_rescue`).
  - So the MVP pattern is: insert the deterministic text immediately (zero added
    latency, unchanged behavior), then asynchronously run the LLM, validate, and —
    only if it passes — offer/apply the polished version. This is why the feature
    can't regress the headline release-to-text latency.
- **The verbatim-fidelity guardrail primitives already exist.**
  - `asr_integrity_metrics(raw, final) -> (percent, raw_count, final_count)`
    (`state.rs:574`) measures token overlap between two transcripts — reuse it as
    an LLM output validator (reject rewrites that drop/alter too many tokens).
  - `stabilize_custom_terms(&text, custom_terms)` (`transcript/mod.rs:51`, impl ~`:522`)
    locks user dictionary terms against mutation — re-run it AFTER the LLM to repair
    any drift on protected terms.
  - The codebase's anti-paraphrase stance is explicit: the faster-whisper literal
    retry prompt says "Do not paraphrase" (grep `Do not paraphrase` in
    `src-tauri/src/inference/mod.rs`) — the LLM rewrite is in direct tension with
    this, which is exactly what the spike must measure.
- **The Python sidecar is the cheap host for the LLM.**
  - Production ASR already runs faster-whisper in a Python worker:
    `src-tauri/windows/faster-whisper/worker.py` (a blocking read-line → dispatch
    loop; commands include `transcribe`, `warmup`, `prefetch`, `shutdown`; models
    gated by an `ALLOWED_MODELS` set near the top). A dev venv exists at
    `.venv-faster-whisper` with CUDA DLL wiring already in `worker.py`.
  - `llama-cpp-python` can load a small instruction-tuned GGUF in this same venv,
    reusing process management + GPU setup. The vendored `whisper-rs` ggml is the
    math library ONLY (no llama.cpp graph/tokenizer) — it CANNOT run an LLM, so a
    new dependency is unavoidable; the sidecar makes `llama-cpp-python` the cheapest
    route (vs. a new Rust llama binding).
- **The model manager can distribute the GGUF.** `is_supported_format` already
  accepts `"gguf"` (`src-tauri/src/model_manager/mod.rs:1284`); the hardcoded
  catalog is built in `build_whispercpp_catalog` (~`model_manager/mod.rs:1462`).
  NOTE: for GGML/GGUF the download deliberately skips checksum (`skip_checksum =
  manifest.format == WCPP_FORMAT`, ~`:620`), trusting HTTPS — for a real ship you'd
  want a genuine hash; for the spike, manual/local model placement is fine.
- **Latency budget**: `docs/testing/hardware-tiers.md` — reference tier targets p95
  ≤ 900 ms end-to-end. An inline LLM would blow this; async post-insert does not
  touch it. The spike measures the LLM's OWN latency separately.
- **ADR**: `docs/adr/0001-phase-0-locked-decisions.md:18` locks "strictly local-only
  with no cloud rewrite path." On-device rewrite is consistent with local-only but
  is ADR-adjacent — a GO decision must ship behind a NEW ADR that supersedes/extends
  0001 (the doc requires ADRs for architecture-impacting changes).

**Conventions to match:**
- Python worker: JSON-over-stdin/stdout, one request → one response line, matched by `id`; a mismatch kills the worker (see `send_worker_request_blocking` in `inference/faster_whisper.rs`). Any new worker command must obey this contract.
- Off-by-default settings: new `VoiceWaveSettings` bools default off (see `code_mode.enabled`, `diagnostics_opt_in`).

## Commands you will need

| Purpose | Command | Expected |
|---|---|---|
| Rust build/test | `cd "C:\Users\posan\OneDrive\Desktop\voice vibe\src-tauri" && export PATH="$PATH:/c/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/Common7/IDE/CommonExtensions/Microsoft/CMake/CMake/bin" && cargo test --lib` | `218 passed`+ |
| Frontend | `cd "C:\Users\posan\OneDrive\Desktop\voice vibe" && npx vitest run && npm run build` | 34 pass, built |
| Python venv | `.venv-faster-whisper\Scripts\python.exe` | the sidecar interpreter (has CUDA DLLs) |

## Scope

**In scope (spike):**
- `plans/artifacts/005-llm-spike.md` (NEW — the evidence log; the primary deliverable)
- A standalone Python prototype script under `scripts/llm-polish/` (NEW — NOT wired into the app yet) to measure quality/latency in isolation.
- ONLY IF Phase 2 gate passes: a minimal, off-by-default prototype path in `src-tauri/src/state.rs` + a new `llm_polish_enabled: bool` setting + a Python worker command, behind a feature flag.

**Out of scope:**
- Turning it on by default. Ever, in this plan.
- Any change to `finalize_pro_transcript` (keep it pure and sync).
- Production model distribution / signing / catalog wiring (that's the follow-up ship plan).
- Regressing or touching the ASR worker's existing commands.

## Git workflow

- Branch: `advisor/005-llm-polish-spike`
- Commit style: `spike(llm): <phase>`; keep prototype commits clearly labeled as spike.

## Phases

### Phase 0: Write the evidence log skeleton

Create `plans/artifacts/005-llm-spike.md` with sections: Model chosen + why, Quality results, Latency results, Guardrail results, GO/NO-GO recommendation. You will fill it as you go. This file is the deliverable.

### Phase 1: Isolated quality+latency prototype (no app changes)

Under `scripts/llm-polish/`, write a standalone Python script (uses `.venv-faster-whisper`; `pip install llama-cpp-python` into that venv — this is a spike, dependency is local-only for now). It:
1. Loads a SMALL instruction-tuned GGUF (pick one that fits the user's GPU VRAM alongside whisper — e.g. a 1–3B instruct model, Q4). Record the exact model + size.
2. Runs a fixed "polish" prompt over a set of ~15 realistic raw dictation transcripts (write them into the script: rambling speech, filler, a name, a number, a code identifier, a URL — the hard cases). The prompt must instruct: fix grammar/punctuation/flow ONLY; never invent, never alter names/numbers/code/URLs; return only the rewritten text.
3. For each, prints: raw, polished, decode latency (ms), and a token-overlap % (reimplement the `asr_integrity_metrics` overlap logic in Python, or eyeball).

**GATE 1 (quality)**: Record in the artifact log. GO only if, across the 15 cases, the model (a) improves readability on the rambling cases AND (b) does NOT alter any name/number/code/URL. If it hallucinates or mangles entities even once in a way a validator couldn't catch, that's a NO-GO signal — record it and STOP with a written recommendation. Do not proceed to app wiring on a failed quality gate.

**GATE 1 (latency)**: Record p50/p95 decode latency for a paragraph. This informs the async UX (a 300ms polish that swaps a beat later is fine; a 5s one is not).

### Phase 2: Validator design (still isolated)

In the same script, add the guardrail the production feature would use:
- Reject the rewrite if token-overlap with the raw transcript is below a threshold (tune it on the 15 cases).
- Detect if any digit-sequence, capitalized proper noun, or fenced/code token present in the raw is MISSING or CHANGED in the rewrite → reject.
- On reject → fall back to the deterministic text.
Record how many of the 15 the validator correctly accepts/rejects.

**GATE 2**: GO to Phase 3 only if the validator reliably catches the bad rewrites while passing the good ones. Otherwise STOP — the feature isn't safe yet; write up what a better validator would need.

### Phase 3: Minimal off-by-default app prototype (only if Gates 1&2 GO)

Wire the smallest possible real path, all behind a new `llm_polish_enabled: bool` setting defaulting to **false**:
1. Add a Python worker command `polish` to `worker.py` (obeying the one-request/one-response `id` contract) that loads the LLM lazily on first use and returns the rewritten text. Keep it in the SAME worker or a second dedicated worker — measure VRAM; if whisper + LLM don't co-fit, use a separate worker process and document the VRAM cost.
2. In `state.rs`, after a successful insertion and only when `llm_polish_enabled`, `tauri::async_runtime::spawn` a task that: calls the polish worker with the just-inserted text, runs the validator (reuse `asr_integrity_metrics` + re-run `stabilize_custom_terms`), and — if it passes — surfaces the polished text via `emit_pill_rescue`-style notice offering to replace (do NOT silently overwrite the user's text in v1 of the prototype; OFFER it). This keeps the deterministic result as the source of truth.
3. Add the setting to `VoiceWaveSettings` (Rust, default false) + TS type + a Settings toggle clearly labeled experimental.

**GATE 3**: Measure on the user's real hardware with real dictation. Record: added latency to release-to-text (must be ~0 since async), VRAM headroom, and whether the offered rewrites are wanted. Write the final GO/NO-GO in the artifact.

**Verify** (Phase 3 must not regress anything): `cargo test --lib` (`218 passed`+), `npx vitest run` (34+), `npm run build` — all green with the flag OFF, proving zero default-path impact.

## Test plan

- The spike's "tests" are the measured gates in the artifact log, not unit tests.
- Phase 3 code, though off by default, must not break existing suites: `cargo test --lib`, `npx vitest run`, `npm run build` all green with `llm_polish_enabled=false`.
- If Phase 3 lands, add ONE Rust unit test for the validator (reject-on-dropped-number, accept-on-clean-rewrite) using canned strings — model after `state.rs` integrity-metric tests.

## Done criteria (spike)

- [ ] `plans/artifacts/005-llm-spike.md` contains: chosen model, quality results on ≥15 cases, latency p50/p95, validator accept/reject results, and a written GO/NO-GO
- [ ] Isolated prototype proves (or disproves) that a small local model can polish without altering entities
- [ ] IF Gates passed: an off-by-default app path exists and all three suites pass with the flag OFF (zero default-path impact)
- [ ] No change to `finalize_pro_transcript` (still pure/sync)
- [ ] `plans/README.md` updated with the spike outcome and, if GO, a pointer to write the productionization plan

## STOP conditions

- The post-insert seam / `CorrectionSession` / `emit_pill_rescue` in `state.rs` don't match the excerpts (drift — these are uncommitted working-tree code; confirm present first).
- Gate 1 quality fails (entity mangling the validator can't catch) → STOP, write NO-GO.
- Gate 2 validator can't separate good/bad rewrites → STOP, write "needs better validator".
- VRAM: whisper + LLM OOM even as separate processes on the target GPU → STOP, record the hardware floor and recommend GPU-tier gating or a smaller model.
- Any attempt to make this default-on, or any latency added to the pre-insert hot path → STOP (violates the plan's core invariant).

## Maintenance notes

- If GO: the productionization plan must add (a) a real signed GGUF catalog entry (not the checksum-skipped `WCPP_FORMAT` path — give it a real hash), (b) a superseding ADR extending `adr/0001` to permit on-device rewrite, (c) VRAM/hardware-tier gating, (d) the "replace vs offer" UX decision, and (e) a way to bound the model download size (a 1–3B Q4 GGUF is ~1–2 GB — meaningful).
- The single biggest ongoing risk is trust: one hallucinated fact in a dictated message erodes it irreversibly. The validator is load-bearing forever; any prompt/model change must re-run the Phase 1/2 case set.
- Reviewer of the spike judges the ARTIFACT (the evidence), not code polish.
