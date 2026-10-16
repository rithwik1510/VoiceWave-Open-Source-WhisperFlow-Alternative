# 0003 - Phase B Runtime Backend Auto Selection and Telemetry Extension

## Status

Superseded by plan 019 (`plans/019-universal-gpu-speech-engine.md`): the in-process whisper-rs path, `backend.rs` and the `whisper-cuda` feature were removed; whisper.cpp now runs in separate speech worker processes. The `backendRequested` / `backendUsed` / `backendFallback` telemetry fields remain.

## Context

`small.en` remained slower than desired on CPU-only inference in real push-to-talk flows. The product direction is zero-config runtime behavior (no extra user tuning), but we still need to exploit available hardware acceleration on Windows.

At the same time, latency diagnostics were missing backend visibility, so we could not prove whether decode was running on CPU, attempted GPU, or fell back.

## Decision

Phase B introduces additive runtime/backend behavior:

1. Add optional Rust feature `whisper-cuda` (`src-tauri/Cargo.toml`) and auto-enable it from Windows Tauri wrappers when CUDA toolkit presence is detected.
2. Add runtime backend policy in `src-tauri/src/inference/backend.rs`:
   - prefer CPU for `tiny`/`base`,
   - prefer CUDA for `small` and above when compiled/allowed,
   - always fall back to CPU on backend init failure.
3. Reuse the same policy for cold decode and pooled runtime decode paths.
4. Extend latency event/diagnostics payloads with additive fields:
   - `backendRequested`
   - `backendUsed`
   - `backendFallback`

No command names are renamed and existing payload fields remain unchanged.

## Consequences

### Positive

1. Runtime can accelerate heavier models automatically when CUDA is available.
2. Non-GPU and misconfigured GPU environments remain functional via CPU fallback.
3. Performance triage now has backend visibility in the same telemetry stream used for reliability gates.

### Tradeoffs

1. CUDA acceleration still depends on local toolkit/build availability in this phase.
2. Heuristic backend selection (model-size based) may need tuning after broader artifact collection.

### Follow-up

1. Collect Phase B live latency artifacts to validate `small.en` gains on RTX-class hardware.
2. Revisit heuristic backend policy and consider metric-driven switching once enough real runs are available.
3. Evaluate broader GPU backend coverage for non-CUDA hardware in later phases.
