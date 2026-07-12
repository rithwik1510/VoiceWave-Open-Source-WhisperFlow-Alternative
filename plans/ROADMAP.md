# Product Roadmap — Track 2 (post-0.5.1)

Captured 2026-07-06 after the v0.5.x release arc (bundled runtime, AI polish,
Quiet Ink + Signal Blue redesign, Updates pane). VoiceWave is now the
maintainer's daily driver; focus shifts to product features. This file is the
product-feature track; engineering fix-tier items stay in project memory
(`production-audit-2026-07`) and executable plans stay in numbered files here.

## Feature candidates (rough value order)

| # | Feature | Effort | Status | Existing plan |
|---|---------|--------|--------|---------------|
| 1 | Spoken edit commands | S | Not started | plans/002 (written, executable) |
| 2 | First-run onboarding | M | Not started | — |
| 3 | Context-aware polish profiles | M | Not started | — |
| 4 | Stats dashboard | S–M | Not started | — |
| 5 | Protected voice snippets | L | Planned | plans/012 (replaces obsolete plan 004) |
| 6 | GPU acceleration pack | L | Parked | — (verified 2026-07-06: bundled runtime is CPU-only for all users; maintainer's GPU speed comes from the dev-venv env-var override) |

### 1. Spoken edit commands (plan 002)
Say "new line", "new paragraph", "bullet point" inline while dictating and get
the formatting, always-on. The transcript pipeline already has the primitives;
plan 002 is fully scoped and executable in about a session. Highest
daily-driver value per unit effort.

### 2. First-run onboarding
A 60-second guided flow on first launch: mic check (reuse the mic-volume-guard
infrastructure), hotkey rehearsal, a "try dictating here" playground box, model
choice. The launch multiplier — converts install traffic into retained users.
Ship before the marketing push, not after.

### 3. Context-aware polish profiles
The insertion engine already classifies the focused app by process (that's how
the Codex/Claude terminal detection works). Feed that classification to the AI
polish pass: full sentences for email clients, casual for chat apps, polish
auto-disabled for terminals/IDEs. Deepens the flagship differentiator; this is
a paid-tier feature at Wispr Flow ("context awareness").

### 4. Stats dashboard
Words dictated, streaks, estimated time saved vs typing. The encrypted
diagnostics store already records every utterance (up to 5000 records with
latency + decode metadata), so this is mostly aggregate-and-render. High
engagement value; screenshots are free marketing.

### 5. Protected voice snippets (plan 012)
Spoken trigger → exact text expansion, with encrypted local-first storage,
cross-device reconciliation, inline matching, and protection from deterministic
formatting and AI polish. This is now the highest-priority product feature and
should ship as a trustworthy personal workflow before team sharing or variables.

### 6. GPU acceleration pack (parked from Track 1)
Settings detects an NVIDIA GPU → offers an on-demand download of the CUDA
libraries (nvidia-cublas-cu12 / nvidia-cudnn-cu12 wheels + a CUDA
llama-cpp-python build) into app-data → workers pick them up automatically.
No installer bloat. Verified facts (2026-07-06): the bundled runtime ships no
nvidia packages; `worker.py::cuda_runtime_libs_ready` gates CUDA on
`cublas64_12.dll` loadability, which only the dev venv satisfies today, so
every normal user decodes on CPU. Both the polish worker (auto-detects
CUDA-capable llama builds) and the ASR worker (auto-registers
`site-packages/nvidia/*/bin`) are already wired to light up if the libraries
appear — delivery is the only missing piece (reuse the polish-model download
pattern).

## Related launch item (not a feature)
- **winget PAT**: `winget install VoiceWave.LocalCore` is the README's primary
  install path and the publish-winget job has failed since v0.5.0 (invalid
  PAT). Fix = classic PAT with `public_repo` scope saved as repo secret
  `WINGET`. Cheap, and stale winget listings undercut a launch.

## Explicitly deferred (do not re-litigate without new evidence)
- Streaming live preview (architecture trap for the faster-whisper worker; see
  plans/README.md rejected section).
- React 19 / Vite 8 / Tailwind 4 upgrades.
- Monetization activation (business timing decision, not engineering).
