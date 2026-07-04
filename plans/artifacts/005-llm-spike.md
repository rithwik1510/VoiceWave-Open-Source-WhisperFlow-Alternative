# 005 — On-device LLM "polish" pass: spike evidence log

> This is the **primary deliverable** of plan 005. It records measured evidence
> and a GO/NO-GO. It is not marketing — write down what actually happened,
> including failures. A NO-GO with clear evidence is a successful spike.

Status: **IN PROGRESS**
Started: 2026-07-04
Executor: Fable (Opus 4.8) overseeing directly + subagents

---

## 0. Hardware & environment (the constraints the spike lives under)

| Fact | Value | Source |
|---|---|---|
| GPU | NVIDIA RTX 3060 Laptop | `nvidia-smi` |
| Total VRAM | 6144 MiB (6 GB) | `nvidia-smi` |
| VRAM free at idle desktop | ~4592 MiB (≈1.4 GB already used by OS/desktop) | `nvidia-smi` |
| Default ASR model | `fw-large-v3-turbo` (stays warm in VRAM between dictations) | settings normalization |
| Est. VRAM held by warm turbo | ~2 GB | ctranslate2 fp16/int8 turbo |
| Python venv | `.venv-faster-whisper`, Python 3.12.6 | — |
| LLM runtime | `llama-cpp-python` 0.3.32, **CPU build** | pip |
| Why CPU, not GPU | Warm whisper-turbo (~2 GB) + a 3B Q4 LLM (~3.5 GB) > 6 GB total → OOM risk. On this GPU the realistic production backend for an *async, post-insert* polish is CPU (no VRAM contention with the ASR that must stay warm). GPU would only be viable with a much smaller LLM or a whisper unload/reload dance (bad UX). | VRAM math |

**Consequence for the spike:** quality (Gate 1) is backend-independent — a model's
rewrite fidelity is identical on CPU or GPU — so CPU measures quality faithfully.
Latency (Gate 1b) is measured on **CPU**, which is the realistic production number
on 6 GB hardware. A note is added on what GPU latency would look like if VRAM allowed.

---

## 1. Model chosen + why

| Field | Value |
|---|---|
| **Model of record** | **Qwen2.5-1.5B-Instruct, Q4_K_M GGUF** (`bartowski/Qwen2.5-1.5B-Instruct-GGUF`, ~1.0 GB) |
| Why 1.5B over 3B | Equal measured quality (14/15) but **more conservative** — it fixes grammar/filler while staying close to the spoken words, whereas the 3B rephrases more freely (e.g. "I don't think we should" → "We should not", a tone shift). For a dictation tool where fidelity is the point, conservative is correct. Also 2× faster on CPU and 1 GB smaller to download. Decision confirmed by the maintainer. |
| 3B | **Dropped.** (Was the quality-ceiling candidate; the extra fluency came with more paraphrasing liberty the product doesn't want.) |

**Runtime build note:** the prebuilt `llama-cpp-python` CPU wheel crashed on load
(`0xc000001d` illegal instruction) — it was compiled with AVX-512, which this
Ryzen 5800H (Zen 3) lacks. Fixed by building from source targeting AVX2/FMA
(`scripts/llm-polish/build_llama.bat`). **Caveat:** the source build bumped
`numpy` 2.4.2→2.5.0 in the shared `.venv-faster-whisper`; faster-whisper +
ctranslate2 still import and run fine, but **production must isolate the LLM in
its own venv** to avoid dep drift into the ASR worker.

---

## 2. Quality results (GATE 1)

Run `scripts/llm-polish/results/run-20260704-010952.json` — Qwen2.5-3B-Instruct
Q4_K_M, temp=0, deterministic.

**Score: 14/15 preserved every protected entity.** The rewrites are genuinely
good, not just "grammatically nudged":

- **Filler / false starts** removed cleanly. Case 11 `"i think we should i think we
  should actually no we should ship the fix on monday not tuesday"` →
  `"We should ship the fix on Monday, not Tuesday."`
- **Run-on list** (case 12) correctly comma-separated.
- **Over-editing guard held**: case 13 (already-clean input) came back byte-identical.
  No hallucinated additions on any clean case.
- **Entities preserved verbatim** across names (Siobhan Ng, Marcus), a phone number
  (415 555 0147), camelCase + snake_case code (`getUserProfile`, `fetch_user_data`,
  `auth_token`, `maxRetries`), a function call (`transcribe()`), a URL, an email, a
  date/time (9:30 AM Eastern), money/percent (1.4 / 23), and a version (2.0).

**The 1 failure is the key finding (case 8, file path):**
```
in : C:\Users\rith\AppData\voicewave\diagnostics.json
out: C:\Users\rith\AppData\Local\VoiceWave\diagnostics.json
```
The model applied *world knowledge* to "correct" a path it should have copied
verbatim (inserted `Local\`, capitalized `VoiceWave`). This is the archetypal
trust-breaking failure mode. **It matters that it happened — and it matters more
that the validator caught it** (see §3). It proves (a) small models DO occasionally
mangle entities, and (b) the guardrail is load-bearing and works.

**Soft observation (not a failure):** case 7 injected a `$` before "1.4 million"
(unspoken but sensible). The validator passed it (both numbers intact, no invented
digits). A strict-fidelity mode could suppress this; most users would want it.

**Gate 1 verdict (quality): GO.** A small local model can polish real dictation
without mangling entities in the overwhelming majority of cases, and its rare
failure is an entity alteration — exactly what a validator can detect.

## 2b. Latency + model-size comparison (GATE 1b) — CPU, idle machine

| Model | Quality | Latency p50 / p95 | Load | Validator false-accepts | Validator false-rejects |
|---|---|---|---|---|---|
| Qwen2.5-**1.5B** Q4_K_M | 14/15 | **1.55 s / 3.05 s** | 1.5 s | 0 | 0 |
| Qwen2.5-**3B** Q4_K_M | 14/15 | 3.28 s / 4.10 s | 2.6 s | 0 | 1 (safe) |

- The first 3B run reported p95 11.5 s, but that overlapped a `cargo` compile.
  The **idle** 3B p95 is **4.1 s** — the contention caveat was confirmed.
- **On this 15-case set, 1.5B matches 3B on quality (14/15) at ~2× the speed** and
  had zero false rejects. Its one miss was a snake_case identifier (`auth_token`),
  different from 3B's file-path miss — **both caught by the validator**.
- **1.5B at p50 1.5 s is genuinely shippable** for an async, offer-a-beat-later UX.
- **Honest caveat:** 15 cases is too small to declare 1.5B == 3B in general. Smaller
  models fail more at the tails. Prototype with 1.5B (latency); keep 3B as the
  higher-assurance option; settle the choice on a larger corpus before default-on
  (which this plan never does anyway).
- **UX consequence:** fine for async post-insert; too slow for any synchronous/inline
  use (which the architecture already forbids).

### GPU (measured on the RTX 3060, after building llama-cpp-python with CUDA)

The CPU-only conclusion above was driven by the 3B's size. With the chosen **1.5B**,
GPU is viable and much faster. Built a CUDA wheel (`scripts/llm-polish/build_llama_cuda.bat`;
Ninja generator — the MSBuild path fails with "No CUDA toolset found"). Measured:

| Backend | Latency p50 / p95 | Notes |
|---|---|---|
| CPU (Ryzen 5800H) | ~1.55 s / 3.05 s | reliable, ships everywhere |
| **GPU (RTX 3060, full offload)** | **~160 ms / 198 ms** | ~10× faster; polish is near-instant |

**VRAM co-residency with the warm ASR model (the production scenario):**

| Resident | VRAM used | Free (of 6144 MiB) |
|---|---|---|
| desktop baseline | 1531 | 4464 |
| + whisper turbo (int8_float16) | 2652 | 3343 |
| **+ 1.5B formatter (both)** | **3978** | **~2017** |

Whisper turbo is ~1.1 GB, the 1.5B ~1.3 GB; both co-fit on 6 GB with ~2 GB headroom.
The worker now auto-selects GPU vs CPU (`_resolve_gpu_layers` in `polish_worker.py`),
gated on a CUDA-capable llama build + a free-VRAM check (≥1800 MiB), mirroring the ASR
worker's GPU gating. **Production caveat:** distributing a CUDA-enabled `llama-cpp-python`
to end users is non-trivial (CUDA runtime/driver matrix) — ship the CPU build for all,
offer GPU as auto-detected acceleration where available.

---

## 3. Validator results (GATE 2) — measured in the same run

The guardrail (`validator.py`) runs with **no per-case ground truth** — only raw +
rewrite, exactly as production would.

| Outcome | Count | Meaning |
|---|---|---|
| Correctly accepted good rewrites (TN) | 13 | polished text offered |
| **Caught the bad rewrite (TP)** | **1** | case 8 path mangling → rejected → deterministic text kept |
| **False accepts (dangerous)** | **0** | no entity-mangling rewrite slipped through |
| False rejects (safe) | 1 | case 11 perfect rewrite rejected on token-overlap 0.56 < 0.60 |

**Gate 2 verdict (validator): GO.** Zero false accepts is the number that matters —
the guardrail never let a trust-breaking rewrite reach "offer." The single false
reject costs only UX, not trust; lowering the overlap floor to ~0.55 recovers it on
this set without a false accept (case 8 is rejected by the *entity* check at overlap
1.00, so the overlap threshold doesn't gate it). Set the threshold on a larger
corpus, not on 15 cases.

---

## 4. App-path notes (Phase 3, only if Gates 1&2 GO)

**Status: LANDED (off by default, uncommitted working tree).** Plan
`plans/005-phase3-wiring.md` executed. The minimal real app path is wired and all
suites are green with the flag at its default (`llm_polish_enabled = false`):
`cargo test --lib` **226 passed** (224 existing + 2 new validator tests),
`npx vitest run` **34 passed**, `npm run build` clean.

What shipped:
- **Thin CPU polish worker** `src-tauri/windows/llm-polish/polish_worker.py` —
  mirrors the faster-whisper worker's stdin/stdout JSON-line loop, lazy-loads the
  GGUF on first `polish` (`n_ctx=2048, n_gpu_layers=0`), uses the spike's
  verbatim `SYSTEM_PROMPT` + `clean_output`. Smoke test:
  `{"id":1,"command":"polish","text":"um so we should ship on monday"}` →
  `{"id":1,"ok":true,"text":"We should ship on Monday."}`.
- **Separate Rust worker client** `src-tauri/src/inference/llm_polish.rs` — its OWN
  `POLISH_WORKER` static + `POLISH_GATE`; never touches the ASR `WORKER` /
  `WORKER_REQUEST_GATE`. `polish_text()` is best-effort: any error → `Ok(None)`.
- **Rust fidelity validator** — `mod polish_validator` inside `state.rs` (reuses
  `asr_integrity_metrics`), a manual-string-scanning port of `validator.py` with
  **no `regex` crate added**. Checks: token overlap ≥ 0.55, protected-entity
  preservation, no invented numbers.
- **Async post-insert hook** in `state.rs` — inside
  `if insertion_success && settings.llm_polish_enabled`, spawns a background task
  AFTER insertion (zero added release-to-text latency), re-runs the dictionary
  stabilizer, validates, and only then OFFERS the rewrite via the existing pill
  `copyTranscript` action. Never overwrites the deterministic text.
- **Settings + experimental toggle** — `llm_polish_enabled` (`#[serde(default)]`,
  Default false) + optional `llmPolishEnabled?` in TS + one labeled Settings toggle
  ("On-device AI polish (experimental)").

Confirms the §5 conditions: off by default (1), offer-don't-overwrite (2),
validator load-bearing (3), separate worker process with its own gate (5). VRAM:
worker runs CPU-only (`n_gpu_layers=0`), so no co-residency with warm whisper.
Still open for productionization: venv isolation (4) — the prototype reuses the
faster-whisper venv/interpreter — and signed GGUF distribution + ADR (6).

---

## 5. GO / NO-GO recommendation

**GO — build the off-by-default prototype (Phase 3).** The spike answered its
research question with data:

- **Can a small local model polish real dictation without mangling entities?**
  Yes — 14/15 clean on hard cases, for both 1.5B and 3B, with the over-editing guard
  holding on clean input.
- **When it fails, is the failure catchable?** Yes — every failure was an entity
  alteration (a path, a code token), and the no-ground-truth validator caught it.
  **Zero false accepts** across 45 total case-runs (3B×2 + 1.5B). That is the number
  that decides trust.
- **Is it fast enough to not hurt UX?** Yes, because it's async post-insert (zero
  added release-to-text latency by construction), and even the polish itself is
  p50 1.5 s (1.5B) — a responsive "offer a beat later."
- **Does it fit the hardware?** Yes, on **CPU** — which side-steps the 6 GB-VRAM
  co-residency problem with warm whisper entirely. GPU offload is a later
  optimization, not a requirement.

**What GO does NOT mean:** it is not default-on, not a shipped feature, and not a
settled model choice. It means the risk that killed similar ideas (silent entity
hallucination) is measurably contained by the validator, so a guarded prototype is
justified.

### Conditions any productionization MUST carry (for the follow-up ship plan)
1. **Off by default**, opt-in, clearly labeled experimental.
2. **Offer, don't overwrite** — deterministic ASR text stays the source of truth;
   the polished version is presented, not silently swapped, in v1.
3. **Validator is load-bearing forever** — any prompt/model change re-runs the case
   set; expand the corpus well beyond 15 before trusting a threshold.
4. **Isolate the LLM venv** from the ASR worker (the spike bumped numpy in the shared
   venv — acceptable for a spike, not for ship).
5. **Separate worker process** for polish (never through the ASR `WORKER_REQUEST_GATE`,
   or it serializes against the next dictation).
6. **Superseding ADR** extending `adr/0001` to permit on-device (not cloud) rewrite,
   plus real signed GGUF distribution and a bounded (~1 GB) download.

### Phase 3 prototype status
See §4. Model of record: **Qwen2.5-1.5B-Instruct Q4_K_M** (3B dropped).

### Follow-up hardening (2026-07, post-Phase-3)
1. **Model locked to 1.5B** (conservative rewrites; maintainer decision).
2. **GPU auto-detect** in `polish_worker.py` (`_resolve_gpu_layers`): CUDA-capable llama
   build + free-VRAM ≥1800 MiB → full GPU offload (~160 ms), else CPU (~1.5 s). Mirrors
   the ASR worker's GPU gating. Env overrides `VOICEWAVE_POLISH_FORCE_CPU/FORCE_GPU`.
3. **Polarity guard** added to `validate_polish` (state.rs): negation-marker count must be
   preserved (raw vs polished, after `n't`→` not` expansion). Closes the one meaning-drift
   hole the token-overlap floor can't catch (negation-only flip). +2 Rust unit tests.
4. **Robustness:** polish worker spawn `Stdio::piped()`→`Stdio::null()` for stderr (CUDA
   init is chatty; an undrained pipe could deadlock the worker).
5. **Regression corpus:** `scripts/llm-polish/polish_spike.py` (15 entity cases) +
   `semantic_probe.py` (20 negation/modal cases) — rerun on any prompt/model change.

Suites after hardening: `cargo test --lib` **228 passed** (+2 polarity), worker GPU
smoke test returns clean, negation-preserving output. All still off by default.
