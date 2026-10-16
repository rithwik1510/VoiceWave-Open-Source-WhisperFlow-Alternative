# Phase A CPU Latency Artifacts

Historical CPU latency evidence from the in-process whisper.cpp runtime
(`cpu-latency-YYYY-MM-DD.json`). That runtime and its sweep generator were
removed in plan 019 (whisper.cpp now runs in the speech worker; benchmark it
with `scripts/bench/`). `scripts/phaseB/run-phaseB-live-fw-report.ps1` still
reads the latest artifact as a baseline.

Expected JSON shape:

```json
{
  "generatedAtUtc": "2026-02-11T00:00:00Z",
  "releaseToTranscribingP95Ms": 250,
  "cacheHitRatio": 0.8,
  "decodeFailureRate": 0.0,
  "emptyDecodeRate": 0.0,
  "longUtteranceTailLossCount": 0,
  "smallEn": {
    "p50ReleaseToFinalMs": 4000,
    "p95ReleaseToFinalMs": 5000
  }
}
```
