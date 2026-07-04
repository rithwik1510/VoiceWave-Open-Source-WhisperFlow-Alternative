# Plan 005 — Phase 3: minimal off-by-default LLM polish app path

> **Context**: Phases 1 & 2 of the LLM-polish spike PASSED (see
> `plans/artifacts/005-llm-spike.md`): a small local model polishes real dictation
> at 14/15 entity fidelity, and the validator caught the one failure with **zero
> false accepts**. This plan wires the smallest REAL, **off-by-default** path into
> the app to prove it end-to-end. It must not change any default behavior.
>
> **Executor**: follow step by step, run every verification, honor STOP conditions.
> Work in the MAIN working tree (do NOT worktree/stash/commit). Leave changes
> uncommitted for review. Plans 001 and 003 already landed in this tree.
>
> **Drift note**: 001 and 003 shifted line numbers in `state.rs`/`lib.rs`/
> `settings/mod.rs`/`voicewave.ts`. Use grep anchors below, not absolute lines.

## Non-negotiable invariants (violating any = STOP)

1. **Off by default.** New setting `llm_polish_enabled: bool` defaults **false**.
   With it false, behavior is byte-identical to today, and all suites pass.
2. **Never on the pre-insert hot path.** The polish call happens only in an async
   task spawned AFTER a successful insertion. Zero added release-to-text latency.
3. **Never through the ASR worker gate.** The polish worker is a SEPARATE process
   with its OWN static handle + request gate. It must not touch
   `WORKER_REQUEST_GATE` or the ASR `WORKER` in `inference/faster_whisper.rs`.
4. **Offer, do not overwrite.** The deterministic inserted text stays the source of
   truth. The polished version is surfaced in the pill with a Copy button (reuse the
   `copyTranscript` action from plan 003). No automated re-insertion in this phase.
5. **Validator is mandatory.** A rewrite is surfaced only if it passes the Rust
   validator (token overlap + entity preservation + no invented numbers). On any
   validation failure, silently drop it (keep the deterministic text, no pill).

## Commands

| Purpose | Command | Expected |
|---|---|---|
| Rust | `cd "C:\Users\posan\OneDrive\Desktop\voice vibe\src-tauri" && export PATH="$PATH:/c/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/Common7/IDE/CommonExtensions/Microsoft/CMake/CMake/bin" && cargo test --lib` | `224 passed`+ |
| Frontend | `cd "C:\Users\posan\OneDrive\Desktop\voice vibe" && npx vitest run && npm run build` | 34+, built |

## Scope (in)

- `src-tauri/windows/llm-polish/polish_worker.py` (NEW — thin CPU polish worker)
- `src-tauri/src/inference/llm_polish.rs` (NEW — Rust worker client, mirrors `faster_whisper.rs`)
- `src-tauri/src/inference/mod.rs` (register `pub mod llm_polish;` if inference is a module dir; else add module where siblings live)
- The Rust validator (Step 3): a private `mod polish_validator` inside `state.rs`, next to `asr_integrity_metrics` so it can call it directly. NO new dependency.
- `src-tauri/src/settings/mod.rs` (add `llm_polish_enabled: bool`, `#[serde(default)]`, Default false)
- `src-tauri/src/state.rs` (the async post-insert hook + validation + pill offer)
- `src/types/voicewave.ts` (add optional `llmPolishEnabled?: boolean`)
- `src/App.tsx` (one experimental toggle in Settings, clearly labeled)

## Scope (out)

- Turning it on by default (ever).
- Any change to `finalize_pro_transcript` (stays pure/sync) or the ASR worker.
- Production model distribution/catalog/signing, VRAM/GPU offload, auto-replace UX.
- `useVoiceWave.ts` settings-object plumbing if avoidable — make the TS field
  OPTIONAL so the toggle can write it without forcing a full settings refactor;
  if the settings object is strongly typed and requires the field, add it with a
  `false` default at the single construction site and nowhere else.

## Step 1 — The Python polish worker (thin, CPU)

Create `src-tauri/windows/llm-polish/polish_worker.py`. Model it on the
`main()` loop of `src-tauri/windows/faster-whisper/worker.py` (read a line →
`json.loads` → dispatch on `command` → print a JSON response line with
`flush=True`; on error print `{"ok": false, "error": ...}`). It must:

- On start, print `{"ready": true}` and flush.
- Resolve the model path from env `VOICEWAVE_POLISH_MODEL_PATH` (set by the Rust
  spawner). If unset/missing, respond to `polish` with `{"ok": false, "error":
  "model not found"}` (do NOT crash).
- Lazy-load the GGUF on first `polish` via `from llama_cpp import Llama`
  (`n_ctx=2048`, `n_gpu_layers=0`, `verbose=False`). Cache it.
- Command `polish`: input `{"id": <n>, "command": "polish", "text": "<raw>"}`.
  Run `create_chat_completion` with temperature 0.0, `max_tokens=400`, using the
  EXACT system prompt from `scripts/llm-polish/polish_spike.py` (copy the
  `SYSTEM_PROMPT` string verbatim — it is the fidelity contract). Echo the `id`
  back. Respond `{"id": <n>, "ok": true, "text": "<polished>"}`. Strip preamble/
  surrounding quotes like `clean_output` in the spike script.
- Command `shutdown`: print `{"ok": true, "shutdown": true}` and exit.

**Verify**: `.venv-faster-whisper\Scripts\python.exe src-tauri/windows/llm-polish/polish_worker.py` then paste one line of stdin:
`{"id":1,"command":"polish","text":"um so we should ship on monday"}` (set
`VOICEWAVE_POLISH_MODEL_PATH` to `scripts/llm-polish/models/Qwen2.5-1.5B-Instruct-Q4_K_M.gguf`
first) → returns `{"id":1,"ok":true,"text":"We should ship on Monday."}`-ish. Paste
`{"command":"shutdown"}` to exit.

## Step 2 — The Rust polish worker client

Create the worker client (new file, e.g. `src-tauri/src/inference/llm_polish.rs`;
register with `pub mod llm_polish;` next to the other inference modules). Mirror the
STRUCTURE of `inference/faster_whisper.rs` but with a completely SEPARATE static
worker + gate so it can never contend with ASR:

- `static POLISH_WORKER: OnceLock<Mutex<Option<PolishWorkerProcess>>>` and a
  private `static POLISH_GATE: Mutex<()>` — do NOT reuse `WORKER`/`WORKER_REQUEST_GATE`.
- `struct PolishWorkerProcess { child, stdin, stdout: BufReader<ChildStdout> }`.
- `spawn_polish_worker()`: resolve the venv python the SAME way `faster_whisper.rs`
  resolves its interpreter (find and reuse that resolution helper — grep how
  `spawn_worker` builds its `Command`; factor a shared helper or duplicate minimally).
  Spawn `python polish_worker.py` with `Stdio::piped()` stdin/stdout, and set env
  `VOICEWAVE_POLISH_MODEL_PATH` to the resolved model path (see Step 4). Read the
  first `{"ready":true}` line.
- `pub async fn polish_text(raw: String) -> Result<Option<String>, ...>`: wrap a
  blocking `spawn_blocking` send (write JSON + `\n` + flush, read one line, parse,
  check `id` match — on mismatch, kill+respawn like the ASR client does). Use a
  generous timeout (e.g. 20 s). Return `Ok(None)` on any worker error (feature is
  best-effort; never propagate a hard error to the caller).

**Verify**: `cargo build --lib` → exit 0. (Runtime exercise happens in Step 5/manual.)

## Step 3 — The Rust validator (port of `validator.py`, NO regex)

**Do NOT add the `regex` crate.** It is deliberately not a dependency — the codebase
does all text processing with manual string methods, and this is off-by-default
prototype code. Port the validator with token-boundary scanning instead. This is
also SAFER: an over-broad "protected token" detector produces more (safe) false
rejects, never false accepts.

Add a Rust function `validate_polish(raw: &str, polished: &str) -> bool` plus
helpers. Put it where it can reuse `asr_integrity_metrics` — a private
`mod polish_validator` inside `state.rs` is simplest (state calls it directly).

Checks (reject = return false on any):

1. **Token overlap ≥ 0.55**: call the existing `asr_integrity_metrics(raw, polished)`
   (`state.rs`, returns a percent + counts); reject if the overlap percent < 0.55.
   (Read its exact return shape and use the overlap field.)
2. **Entity preservation**: extract "protected tokens" from `raw` and require each
   to appear (case-insensitive substring) in `polished`. A token is protected if,
   after trimming surrounding punctuation `.,;:!?()"'`, it satisfies ANY of:
   - contains an ASCII digit (numbers, versions, times, money, phone), OR
   - contains any of `_` `/` `\` `@` (snake_case, paths, emails), OR
   - contains `::` or `()` (code), OR
   - has an internal capital: some index `i>0` where `t[i]` is uppercase and
     `t[i-1]` is lowercase (camelCase), OR
   - contains an internal `.` with alphanumerics on both sides (URLs, filenames,
     `voicewave.dev`, `diagnostics.json`, `2.0`).
   Split on whitespace to get tokens. If any protected raw token is missing from
   `polished` → reject.
3. **No invented numbers**: for every whitespace token in `polished` that contains
   a digit (trim surrounding punctuation first), require it to appear
   (case-insensitive substring) in `raw`. A digit-bearing token in the rewrite that
   isn't in the raw = a fabricated number/version → reject.

Add **2 unit tests** (model after existing `state.rs` tests): (a) `validate_polish`
REJECTS a rewrite that drops/changes a number (raw `"set the limit to 5"`, polished
`"set the limit to five"`) and (b) ACCEPTS a clean filler-removal rewrite (raw
`"um so we should ship on monday"`, polished `"We should ship on Monday."`). Canned
strings only — no model calls.

**Verify**: `cargo test --lib` → `226 passed`+ (existing 224 + 2 new).

## Step 4 — Model path resolution

The Rust spawner must set `VOICEWAVE_POLISH_MODEL_PATH`. For this prototype:
resolve, in order: (1) env `VOICEWAVE_POLISH_MODEL_PATH` if already set; (2) a
repo-relative dev path `scripts/llm-polish/models/Qwen2.5-1.5B-Instruct-Q4_K_M.gguf`
resolved from the current exe/CWD; (3) if none exist, the worker returns "model not
found" and `polish_text` yields `Ok(None)` → feature no-ops. Document that
production distribution is a follow-up (catalog + signed GGUF). Do NOT hardcode an
absolute user path.

## Step 5 — The async post-insert hook in `state.rs`

Find the insertion-success block where `CorrectionSession { inserted_text,
inserted_at_utc_ms }` is set (grep `CorrectionSession {`). Immediately after that,
add:

```rust
if settings.llm_polish_enabled {
    let app_handle = app.clone();
    let raw = inserted_text.clone();            // the text just inserted
    let custom_terms = /* the same terms passed to finalize */;
    tauri::async_runtime::spawn(async move {
        match crate::inference::llm_polish::polish_text(raw.clone()).await {
            Ok(Some(polished)) => {
                // re-run the dictionary-term stabilizer on the LLM output, then validate
                let stabilized = /* stabilize_custom_terms(&polished, &custom_terms) */;
                if validate_polish(&raw, &stabilized) && stabilized.trim() != raw.trim() {
                    emit_pill_rescue(
                        &app_handle, "info",
                        "Polished version ready",
                        None,
                        stabilized,                 // travels as the rescue transcript
                        Some("copyTranscript"),     // reuse the plan-003 copy action
                    );
                }
            }
            _ => {} // best-effort: worker error or rejected rewrite => keep deterministic text
        }
    });
}
```

Adapt names to the real locals in scope (the exact identifiers for the inserted
text, `app`, settings, and custom terms — read the surrounding block). Confirm
`emit_pill_rescue`'s current signature after plan 003 (it now builds a `PillAction`
from the `Some("copyTranscript")` arg — verify by reading it) and call it exactly.
Do NOT block; the spawn returns immediately.

**Guard**: this whole block is inside `if settings.llm_polish_enabled` so with the
flag off (default) nothing spawns and behavior is unchanged.

**Verify**: `cargo test --lib` → `226 passed`+ (no regressions; new validator tests included).

## Step 6 — Settings + toggle

- `settings/mod.rs`: add `pub llm_polish_enabled: bool` with `#[serde(default)]`;
  set Default to `false` (match how `pill_action_suggestions`/`code_mode.enabled` do it).
- `src/types/voicewave.ts`: add `llmPolishEnabled?: boolean` (optional).
- `src/App.tsx`: add one toggle in Settings (near other experimental/advanced
  toggles) labeled e.g. **"On-device AI polish (experimental)"** with a one-line
  description: "After dictation, a local model offers a cleaned-up version in the
  pill. Off by default; nothing is sent to the cloud." Wire it to read/write
  `llmPolishEnabled` through whatever settings-update path the neighboring toggles use.

**Verify**: `npx vitest run` (34+), `npm run build` (clean).

## Test plan

- Rust: the 2 `validate_polish` unit tests (Step 3). All existing suites green.
- Frontend: suite stays green; build clean.
- **Manual (report, do not automate):** with the flag ON and the 1.5B model present,
  do one real dictation and confirm the pill offers a polished version a beat later
  with a working Copy button, and that the deterministic text is untouched. If you
  cannot run the app, say so and rely on the worker smoke test from Step 1.

## Done criteria

- [ ] With `llm_polish_enabled=false` (default): `cargo test --lib` (226+), `npx vitest run` (34+), `npm run build` all green — proving zero default-path impact.
- [ ] Polish worker smoke test (Step 1) returns a clean rewrite.
- [ ] `validate_polish` rejects a dropped-number rewrite and accepts a clean one (unit tests).
- [ ] The async hook is inside `if settings.llm_polish_enabled` and spawns off the hot path.
- [ ] Polish path never touches `WORKER_REQUEST_GATE`/ASR `WORKER`.
- [ ] `finalize_pro_transcript` unchanged.
- [ ] No out-of-scope files modified; `plans/README.md` + `plans/artifacts/005-llm-spike.md` §4 updated with what shipped.

## STOP conditions

- The `CorrectionSession {` insertion-success block or `emit_pill_rescue` signature
  doesn't match after 001/003 (drift) — read them, and if the shape is materially
  different from this spec, STOP and report so the overseer can adjust.
- You believe you need a new crate dependency (e.g. `regex`) — STOP and report; the
  validator is specified to need none.
- Any verification fails twice after a reasonable fix — STOP and report.
- You find yourself needing to touch `finalize_pro_transcript`, the ASR worker, or
  the ASR gate — STOP; the design forbids it.
