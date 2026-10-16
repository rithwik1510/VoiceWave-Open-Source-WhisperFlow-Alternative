# Plan 019: Universal GPU Speech Engine

## Status

**Phases 0–3 implemented (2026-10-16); Phase 4 (real-app walkthrough and
hardware validation) pending.** Commits: 3b85897, ecce249, c35bbed (Phase 0);
0c949c7, e95a538, b97d1c9, 1464434 (Phase 1); 76f6bba, 1805214, 0e855a6
(Phase 2); 4a37d5e (Phase 3).

**v3 approved by the owner (2026-10-16); implementation started** on branch
`fix/first-run-setup-paths`. Owner answered all open questions with the
recommended option (yes to 1, 2, 4, 5). No Intel or AMD-discrete test hardware
is available (question 3): verify driver-specific assumptions against upstream
docs during implementation, and rely on the CI runner as the clean-machine
check.

Review history: v1 and v2 were each reviewed by GPT-6.1-Sol and Claude Fable
5.1. v2 fixed the two v1 blockers (in-process Vulkan, CPU regression). v3
closes the round-2 findings: CPU eligibility, worker build/packaging, the
full deadline and protocol contract, transient-failure handling, and a
model-free device list for the first recommendation. Change tables are at the
end.

Owner decisions:

- Every machine, low end to high end, gets the fastest engine it can run, with
  zero user setup: no driver installs, no extra downloads beyond the speech
  model.
- **whisper.cpp + Vulkan runs as a separate worker process**, never inside the
  app.
- **GPU path only where it qualifies.** Machines without a qualifying GPU keep
  **faster-whisper on CPU**, today's engine, unchanged.
- **No bundled probe model.** The first recommendation comes from hardware
  information (CPU features, plus the GPU list the Vulkan worker reports
  without a model). A test with the user's real model confirms it after
  download.
- Drop the CUDA add-on (the 1.3 GB cuBLAS/cuDNN download in `gpu_runtime.rs`).
- Model choice (Small vs Large Turbo) stays separate from engine choice.

## Goal

Every user gets the fastest speech recognition their hardware can deliver,
automatically:

| Machine | Engine | Model recommended | Expected per-dictation time |
|---|---|---|---|
| Discrete GPU (any vendor, ≥ 4 GB video memory, Vulkan 1.2+) | whisper.cpp + Vulkan worker | Large Turbo | ~0.2–0.5 s |
| Discrete GPU with 2–4 GB | whisper.cpp + Vulkan worker | Small | ~0.1–0.4 s |
| Integrated GPU only (Intel / AMD) | faster-whisper CPU now; Phase 5 adds the GPU path where it wins | Small | today's speed; ~1.8× faster on typical 10 s dictations once Phase 5 ships |
| No usable GPU, or CPU without AVX2 | faster-whisper CPU (today, unchanged) | Small | today's speed |

The user never sees "Vulkan", "CUDA", or "engine". They see "Recommended for
this PC" and fast dictation.

### Success criteria

1. Discrete GPU, Large Turbo: warm decode **under 0.5 s** for clips up to 10 s,
   and release-to-text (includes capture tail and insertion, measured
   separately) under 0.8 s.
2. Machines that stay on faster-whisper: **no change in behaviour or speed**.
3. Quality: word error rate within **+1.0 point absolute** of faster-whisper
   for the same model, on at least 100 clips plus the owner's real dictations.
4. Zero added setup steps. Zero downloads beyond the speech model.
5. A GPU crash, hang, or driver fault never takes down the app and never loses
   a dictation: the audio is kept and transcribed on the CPU rung, inserted
   once. (An OS-level driver reset can still disturb the whole system.)

### Non-goals (this plan)

- Moving AI polish off Python.
- Streaming / partial transcripts.
- New model families (Parakeet, Moonshine, distil-whisper).
- Making whisper.cpp the CPU engine (needs `audio_ctx` work; follow-up).

## Why users run on CPU today

1. **faster-whisper runs on CTranslate2, whose only GPU path is CUDA.**
   NVIDIA-only.
2. **CUDA needs 1.3 GB of extra libraries** (cuBLAS 553 MB + cuDNN 746 MB).
3. A CUDA-linked build almost shipped in 0.5.0 and would have crashed AI polish
   for non-NVIDIA users. The rule became "CPU for everyone, GPU later".
4. Today's GPU check requires `nvcuda.dll` **and** a discoverable
   `cublas64_12.dll`, plus an enabled policy (`faster_whisper.rs:1051-1075`).
   Shipped installs never have cuBLAS, so effectively nobody gets GPU. The
   owner's machine is an exception (env vars point at a CUDA dev venv).

**Why Vulkan solves this:** every modern Intel, AMD, and NVIDIA driver ships
Vulkan (`vulkan-1.dll`). whisper.cpp's Vulkan backend compiles its shaders into
its own binary. Users need nothing new; only our build machine needs the Vulkan
SDK.

## Benchmark results (2026-10-16)

### Method

- Machine: Ryzen 7 5800H (8 cores / 16 threads), integrated AMD Radeon (Vega 8),
  RTX 3060 Laptop GPU, 31 GB RAM. AC/battery state not recorded.
- Engine versions: **whisper.cpp 1.9.5** (built with `GGML_VULKAN=ON`, Vulkan
  SDK 1.4.341.1); faster-whisper 1.2.x. The app's vendored whisper.cpp is
  1.7.6; these numbers apply to 1.9.5, which the worker pins.
- Audio: 20 LibriSpeech clips (test-clean and test-other), 667 reference words,
  in ~2 s, ~5 s, ~10 s, ~30 s buckets (30 s clips are concatenations).
- Settings matched: English, beam 5, temperature 0, no previous-text
  conditioning.
- 1 warmup decode, 3 timed repeats, medians. These are **warm, back-to-back
  decode times**, not release-to-text latency.
- Scripts and raw results: `%TEMP%\vwgpu` (`scripts/`, `results/*.jsonl`,
  `clips/manifest.json`). Summary: `python -I scripts/analyze.py results
  clips/manifest.json`. Phase 0 moves these into `scripts/bench/`.

### Results

| Engine | Device | Model | 2 s | 5 s | 10 s | 30 s | WER |
|---|---|---|---|---|---|---|---|
| faster-whisper int8 (**shipped today**) | CPU | Small | 1.83 s | 2.83 s | 4.34 s | 8.41 s | 3.60% |
| whisper.cpp | CPU (8 threads) | Small | 2.82 s | 2.71 s | 3.48 s | 7.99 s | 3.15% |
| whisper.cpp Vulkan | Radeon iGPU | Small | 1.71 s | 1.94 s | 2.44 s | 7.14 s | 3.30% |
| whisper.cpp Vulkan | Radeon iGPU | Turbo | 5.99 s | 6.13 s | 7.11 s | 9.00 s | 1.20% |
| faster-whisper CUDA f16 | RTX 3060 | Small | 0.16 s | 0.22 s | 0.36 s | 0.93 s | 3.00% |
| faster-whisper CUDA f16 | RTX 3060 | Turbo | 0.31 s | 0.35 s | 0.38 s | 1.09 s | 2.10% |
| whisper.cpp Vulkan | RTX 3060 | Small | 0.12 s | 0.17 s | 0.35 s | 0.92 s | 2.85% |
| whisper.cpp Vulkan | RTX 3060 | Turbo | 0.19 s | 0.25 s | 0.45 s | 1.08 s | 2.55% |

| Config | Load | First decode after load | Peak RAM | GPU memory |
|---|---|---|---|---|
| faster-whisper CPU Small | 1.2 s | 3.3 s | 707 MB | – |
| whisper.cpp CPU Small | 0.9 s | 4.3 s | 870 MB | – |
| whisper.cpp Vulkan iGPU Small | 1.1 s | 5.4 s | 1,097 MB | 841 MB (shared RAM) |
| whisper.cpp Vulkan iGPU Turbo | 2.6 s | 12.1 s | 2,227 MB | 2,064 MB (shared RAM) |
| faster-whisper CUDA Turbo | 3.1 s | 0.8 s | 1,714 MB | 2,419 MB |
| whisper.cpp Vulkan RTX Turbo | 3.4 s | 0.7 s | 1,115 MB | 1,903 MB |

Not measured (owner decision): Large Turbo on plain CPU (earlier: 7–9 s).

### What the numbers say

1. **On a discrete GPU, whisper.cpp + Vulkan is roughly CUDA-class speed**
   (faster on short clips, slightly slower at 10 s), with less GPU memory and
   no 1.3 GB download. This is the core of the plan.
2. **On CPU, whisper.cpp is 54% slower on 2 s clips** (2.82 s vs 1.83 s) and
   faster from 10 s up. 22% of the owner's 3,280 dictations are under 3 s, so
   CPU-only machines stay on faster-whisper.
3. **Integrated GPU (Small) ties today's CPU at 2 s and is 1.5–1.8× faster at
   5–10 s.** The owner's median dictation is 11 s of audio. Worth doing, but
   Intel drivers are untested and it changes those users' model format:
   Phase 5.
4. **Turbo on an integrated GPU is too slow** (~6 s). Never recommended.
5. **Quality:** no evidence whisper.cpp is worse, but 20 clips cannot show
   small differences either way (Vulkan Turbo 2.55% vs CUDA 2.10%; the same
   Turbo model on the iGPU gave 1.20%). Phase 0 re-runs quality on ≥ 100 clips
   before anything ships.
6. **The first decode after loading is slow on Vulkan** (5.4 s iGPU Small,
   12.1 s iGPU Turbo; 0.7 s on the RTX). It must happen during warm-up.

### Caveats

- One laptop, one run, 20 clips of read speech.
- Back-to-back warm decodes. Real dictation has idle gaps and laptop GPUs power
  down; Phase 0 adds "after 60 s idle" and "on battery" runs.
- No Intel GPU and no AMD discrete GPU tested.

## Current state of the code (facts, 2026-10-16)

- **whisper.cpp 1.7.6 is compiled in-process into every build** via
  `whisper-rs 0.15.1` (non-optional, `src-tauri/Cargo.toml:42`), patched to
  `vendor/whisper-rs*`. No whisper.cpp model can be selected, so it is
  dormant.
- Cargo features `whisper-vulkan` / `whisper-cuda` exist (`Cargo.toml:15-19`);
  release passes neither. `run-tauri-build-windows.ps1:179-217`
  (`Resolve-WhisperFeatureArgs`) auto-adds `whisper-cuda` when `CUDA_PATH`
  and `cl.exe` exist.
- `vendor/whisper-rs-sys/build.rs` never sets `GGML_NATIVE` (compiles for the
  CI runner's CPU) and links `vulkan-1` directly (`:229`); ggml initializes
  Vulkan even for CPU contexts (`ggml-backend-reg.cpp:182`). An in-process
  Vulkan build would fail to launch without `vulkan-1.dll`.
- ggml-vulkan has ~45 `GGML_ABORT` / `VK_CHECK` sites; a lost device kills the
  process.
- Device ordering differs by version: vendored 1.7.6 lists discrete GPUs first
  (`ggml-vulkan.cpp:4163-4251`); the 1.9.5 spike listed the Radeon iGPU as
  device 0. Never assume an index.
- Existing in-process GPU guard (`backend.rs:101`, `mod.rs:1545-1581`) covers
  model-load failures only; the executor keeps failed contexts
  (`executor.rs:291-302`).
- **The Python worker transport writes the request synchronously before its
  response timeout starts** (`faster_whisper.rs:630`). A hung worker can block
  the write with no timer running. The new transport must not copy this.
- **Capture and finalization branch by model family:** full capture is kept
  only for faster-whisper (`state.rs:3927`); conditioning and quality handling
  also branch. The outer finalize timeout is at `state.rs:4419`.
- June migration: settings rewrite `wcpp-*` → `fw-*` (`settings/mod.rs:333-344`,
  mirrored in `src/hooks/useVoiceWave.ts:135-139`). Old GGML catalog entries
  are in git (commit `827af46`).
- HTTP model downloader (`model_manager/mod.rs:393`): resume off unless an env
  var is set; checksum skipped for `ggml` because `sha256_hex` reads the whole
  file into RAM (`:648-657`); old file deleted before the new one is verified
  (`:640-644`); `state.rs:2972` holds the `model_manager` mutex for the whole
  download.
- No DXGI or Vulkan enumeration exists; recommendation
  (`model_manager/mod.rs:1419-1450`) uses only the CUDA check.
- `gpu_runtime.rs` is dead except `installed_bin_paths()`;
  `src/lib/gpuRuntime.ts` calls unregistered commands.
- AI polish uses the same bundled Python (`src-tauri/src/inference/llm_polish.rs:458-505`)
  and ships an **AVX2-only** llama wheel; CPU-only.
- CTranslate2 (faster-whisper) requires **at least SSE4.1**.
- Single-instance guard: named mutex in `main.rs:15-66` plus
  `tauri_plugin_single_instance`. Helpers must be different executables.
- Release builds use the GNU toolchain (`x86_64-pc-windows-gnu`).

## Target design

### CPU eligibility (checked first, before any recommendation)

The app checks CPU features with `is_x86_feature_detected!` before
recommending a model or spawning any worker:

| CPU | Outcome |
|---|---|
| No SSE4.1 | Below the app's minimum (CTranslate2 cannot run). Clear "this PC isn't supported" message in setup. |
| SSE4.1 but no AVX2 | faster-whisper CPU only. No whisper.cpp workers, no GGML download. AI polish disabled with an explanation (its wheel needs AVX2). |
| AVX2 | Eligible for everything below. |

No worker is ever spawned on a CPU that would crash it, so there is no
illegal-instruction crash to contain.

### Architecture: a separate speech worker

whisper.cpp runs in its own small executables, built from **pinned upstream
whisper.cpp 1.9.5**:

| Worker | Backends | Used for |
|---|---|---|
| `voicewave-asr-vk.exe` | Vulkan + CPU | GPU dictation, and the model-free device list |
| `voicewave-asr-cpu.exe` | CPU only, no Vulkan anywhere in its dependency chain | Fallback for GPU users when the GPU path fails |

Why this shape:

- **The app never links Vulkan**, so it always launches. A Vulkan worker that
  cannot start is a spawn failure the app handles.
- **Crashes and hangs are contained** to the worker; the app detects them and
  steps down.
- **The CPU fallback contains no Vulkan**, so a broken driver cannot affect it
  (ggml initializes Vulkan at registry time even for CPU work, which is why one
  worker with GPU "off" is not enough). It reads the same GGML file, so falling
  back needs no download.
- **Built with MSVC + CMake in CI**, independent of the app's GNU Rust
  toolchain. Pipes cross no C++ ABI, so mixing toolchains is safe.
- **Not a second copy of `voicewave_core.exe`**, so the single-instance guard
  does not interfere.

#### Build and packaging requirements

- `BUILD_SHARED_LIBS=OFF`: whisper and ggml linked statically into each EXE
  (upstream 1.9.5 defaults to shared libraries under MSVC). No DLLs with
  shared names between variants.
- Static CRT (`/MT`): no dependency on an installed VC++ redistributable.
- `GGML_OPENMP=OFF` (ggml's own thread pool), so no `vcomp140.dll`.
- `GGML_NATIVE=OFF` with explicit AVX2 / FMA / F16C, matching the polish wheel.
- Separate build trees per variant; each worker installed in its own
  directory under the app's resources.
- First lines of the worker: `SetErrorMode(SEM_FAILCRITICALERRORS |
  SEM_NOGPFAULTERRORBOX)` and `_set_abort_behavior(0, _WRITE_ABORT_MSG)`, so a
  crash or `GGML_ABORT` never shows a Windows error dialog.
- whisper.cpp/ggml log output routed to stderr; **stdout carries JSON only**.
- Phase 0 verifies the full dependency chain of both EXEs (`dumpbin
  /dependents`): the CPU worker imports nothing Vulkan, neither imports VC
  runtime DLLs, and both run on a clean machine with no SDK or compiler paths.

Expected installer growth: ~20–35 MB (Vulkan shader blobs dominate), next to
the existing ~535 MB Python runtime. Measured in Phase 0.

#### Worker protocol

JSON lines over stdin/stdout, every request carrying an id that the response
must echo (mismatch = protocol error). Commands:

- `list_devices` — **needs no model**, returns in ~1 s and exits if run with
  `--list-devices`. Per device: name, vendor id, device id, device type
  (discrete / integrated / virtual / cpu), Vulkan API version, memory, LUID
  and UUID (from `VkPhysicalDeviceIDProperties`), and the ggml device index.
- `load` — model path, ggml device index or CPU, flash attention, threads.
- `transcribe` — int16 PCM base64, beam, prompt. Response includes **which
  backend actually ran** (whisper.cpp can silently fall back to CPU).
- `cancel` — abandon the current request.
- Errors carry a class: `artifact` (model file unreadable/corrupt),
  `resource` (out of GPU/system memory), `backend` (device fault), `protocol`.

#### Deadlines (the whole contract, not just decode)

The new transport sends requests from a writer thread so a blocked pipe write
is covered by the same deadline. Every phase has one:

| Phase | Deadline (initial values, tuned in Phase 0) |
|---|---|
| Spawn to ready | 10 s |
| `list_devices` | 5 s |
| `load` (+ warm-up) | 30 s GPU (120 s on first use: driver shader compile, see Phase 0 results), 60 s CPU |
| `transcribe` | GPU: 15 s + 1× audio length for the first 5 dictation decodes after a model load (on a fresh driver shader cache each new audio length compiles shaders first: a 5 s dictation took over 6 s, a 10.8 s one 7.7 s, on the RTX 3060), then 4 s + 0.4× audio length (warm decodes run ~0.07×, so a hang shows in seconds). Qualification clips: 30 s each (a timeout there caches a fail). CPU: 30 s + 3× audio length (CPU rung loads with max(cores − 2, 2) threads) |
| Request write | inside the request's deadline |

On any missed deadline: kill the worker, **reap it** (wait for exit) before
starting another, and retry the dictation on the next rung with the same
retained audio. Text is inserted once, from whichever rung finishes. The outer
finalize timeout (`state.rs:4419`) is raised to cover GPU attempt + CPU retry,
plus the wait for the one-time GPU check when the dictation waits for it
(see Qualification). The first-decodes counter lives on the loaded worker, so
a respawn or model reload starts it over.

### Engine selection

**GPU users** (CPU eligible, and a discrete GPU passes the device rules and
qualification; Phase 5 may add integrated GPUs):

1. `voicewave-asr-vk.exe` on the chosen GPU, GGML model.
2. If the GPU path fails: faster-whisper CPU if a faster-whisper model is
   installed (upgraders have one; faster than whisper.cpp CPU on short clips),
   otherwise `voicewave-asr-cpu.exe` with the same GGML model.

**Everyone else:** faster-whisper on CPU, exactly as today.

### First recommendation (no model needed)

1. CPU eligibility check (above). Not AVX2 → Small, faster-whisper. Done.
2. Cheap pre-check via DXGI: is there any non-software adapter? If not →
   Small, faster-whisper. Done. (Skips spawning the worker on machines with
   nothing to find.)
3. Run `voicewave-asr-vk.exe --list-devices` (~1 s, no model). If it cannot
   start or times out → Small, faster-whisper.
4. Pick the best device by the Vulkan-reported type, not DXGI memory figures
   (DXGI can make an AMD APU's BIOS memory carve-out look like a discrete GPU):

| Best device found | Recommendation |
|---|---|
| Discrete (any vendor, incl. Intel Arc), Vulkan 1.2+, ≥ 4 GB | Large Turbo, GPU path, "Fast on your graphics card" |
| Discrete, Vulkan 1.2+, 2–4 GB | Small, GPU path |
| Discrete below Vulkan 1.2, integrated, virtual, or none | Small, faster-whisper (Phase 5: integrated may qualify) |
| Total system RAM < 8 GB | Small, whatever the GPU |

The chosen device is remembered by persistent identity (vendor id + device id
+ UUID) and matched at runtime by LUID, so identical adapters are not
confused.

### Qualification (the worker's first start with the real model)

No separate probe process. The first time a GPU user's worker starts with
their real model, its warm-up doubles as the qualification test:

1. Spawn `voicewave-asr-vk.exe`, `load` on the chosen device, run one warm-up
   decode (absorbs the slow first-decode pipeline setup).
2. Decode two bundled test clips (~2 s and ~10 s of audio with known
   transcripts; audio, not a model).
3. **Pass** = worker reports the GPU backend actually ran; both transcripts
   within tolerance; warm 2 s decode < 1.0 s **and** warm 10 s decode < 1.5 s
   (discrete); and, where a faster-whisper model is installed (upgraders),
   faster than faster-whisper CPU on the same two clips.
4. On pass, the same worker stays running as the live dictation worker.

Dictation while the check runs (first GPU use: 25–50 s of shader compile):

- **A faster-whisper model is installed** (upgraders): it decodes meanwhile,
  and the step-down notice comes after the result.
- **No faster-whisper model** (new installs): the dictation waits for the
  check, up to its own worst case (spawn + 120 s load + two tries per clip +
  reap, ~4 min; usually 25–50 s) and never past a cancel, then decodes on the
  GPU if it passed, else on the CPU worker. The CPU worker cold-loading Large
  Turbo next to the check took 31.6 s for a 2.6 s utterance and stretched the
  check's GPU load from ~27 s to ~50 s. The "Getting your graphics card
  ready" notice goes out when the wait starts (it replaces "Still
  transcribing…" for that dictation).

This is a **qualification**, not a search for the fastest configuration:
hardware rules decide eligibility, the test confirms the GPU path works and
meets the bar.

Outcomes:

| Result | Action |
|---|---|
| Pass | GPU path on; cached |
| Wrong transcript, GPU not actually used, crash | Fail; cached for this cache key; user goes to their CPU rung. A Fail on this device + driver + worker build with no Pass there (or a driver disable) takes the machine off the GPU path: the catalog offers the faster-whisper models again until a driver or worker update |
| Correct but slow (e.g. on battery, GPU power-gated) | Not cached as a fail; CPU rung this session, retry next session. The 3rd counted retry in a row is cached as Failed |
| `resource` error (GPU memory taken by another app), interrupted check | Inconclusive: never a fail and never counted; CPU rung this session, retry next session |

If a Large Turbo user ends up on CPU, a notice offers switching to Small.

**Cache key:** device identity, driver version, model id + hash, worker build
fingerprint (hash of the worker EXE), and decode configuration. Not the app
version, so app updates do not re-test unless the worker or its settings
changed.

Qualification and polish prewarm are serialized; two large models never load
at once.

### Runtime failure handling

| Event | Class | Action |
|---|---|---|
| Worker exits or misses a deadline during a dictation | backend | Retry that dictation once on a fresh GPU worker (12 s load deadline; its decodes get the cold deadline), then on the CPU rung |
| Worker reports out-of-memory | resource | CPU rung for this dictation; retry GPU on the next dictation; never counts toward disabling GPU |
| Model unreadable or corrupt | artifact | Offer re-download; not a GPU failure |
| Malformed response, id mismatch | protocol | Kill, reap, respawn; counts as a worker bug in diagnostics |
| Second backend failure in a row (a GPU success resets the count) | backend | CPU rung for 5 minutes, then the GPU again |
| A 5-minute cooldown in two sessions on the same driver | backend | GPU disabled for that driver; one pill notice. **Expires** after 7 days or on any app/worker update, then re-qualifies |
| Worker found dead while idle (e.g. after sleep/resume) | – | Respawn lazily on the next dictation; not a failure |
| Cancelled dictation, empty speech | – | Normal |

Background GPU respawn is serialized against the CPU retry and polish so
memory-heavy work never overlaps. Only failures the app observes count; a
reboot or a kill of the whole app leaves no trace.

### Worker residency

Default: keep the GPU worker and its model loaded while the app runs, and let
the driver power-gate the idle GPU. Phase 0 measures the after-idle cost on the
RTX and the VRAM held (~1.9 GB for Turbo). If either is a problem (gamers
noticing held VRAM, or slow after-idle decodes), Phase 0 decides an idle-unload
policy with data.

### Capture and decode integration

The new worker uses **the same capture and finalization path as
faster-whisper** today: full capture kept (`state.rs:3927`), the same audio
conditioning, quality checks, and retry rules. The model-family branches in
`state.rs` become engine-aware ("Whisper model via worker") rather than tied to
`fw-*` ids. Microphone end-to-end runs cover pauses, very short speech, and a
fallback replay (GPU killed mid-dictation, text still inserted once).

### Models

- GGML models from `huggingface.co/ggerganov/whisper.cpp`:
  `ggml-small.en.bin` (~488 MB) and `ggml-large-v3-turbo.bin` (~1.6 GB), with
  real pinned SHA-256 hashes.
- Downloader: streaming hash while downloading; verify the partial file before
  it replaces anything; never delete a working model before its replacement is
  verified; resume on by default; `model_manager` mutex released during the
  download.
- Both model families coexist. Remove the June `wcpp-*` → `fw-*` migration
  (and its frontend mirror). Never rewrite an existing user's active model on
  update.
- User-facing names stay "Small" and "Large Turbo".

### Upgrade path for existing users

- **No GPU path for this machine:** nothing changes.
- **GPU path available:** one notice after the update: "Your graphics card can
  make dictation up to 10× faster — one-time download (1.6 GB)". Dictation
  keeps working on faster-whisper meanwhile.
- After the GGML model installs and qualification passes, the app switches
  atomically. If it fails, they stay on faster-whisper, unaffected.
- Their faster-whisper model stays as the CPU fallback; the Models page shows
  its disk use with a remove option.
- Must survive: declined offer, cancelled or failed download, restart between
  download and switch.

### Settings and UI

- Settings → Dictation: "GPU acceleration: Auto / Off". Off = CPU rung.
- Models page: "Running on NVIDIA GeForce RTX 3060" / "Running on your
  processor", from the backend the worker reports.
- First step-down in a session: one pill notice, plain language. The CPU retry
  of a Large Turbo dictation can take ~10 s; the notice explains why.
- No engine picker.

### Decode settings

- Beam 5 to match the benchmark; tune later with data.
- Flash attention on for discrete GPUs, off for integrated.
- One model per worker, warmed with real audio.

### Diagnostics

Add (local only): engine, device name/type/identity, qualification result and
timings, error class per failure, step-down reason, worker restarts.
`backendUsed` comes from the worker's report.

## Work plan

Each phase ends with a gate. Nothing ships to users until Phase 4 passes.

### Phase 0: de-risk the foundation

1. Worker (`src-tauri/windows/asr-worker/`, C++ + CMake) against pinned
   whisper.cpp 1.9.5: two variants, static linkage, static CRT, OpenMP off,
   explicit CPU baseline, error-mode suppression, JSON-only stdout,
   `--list-devices`.
2. CI job: MSVC + pinned, cached Vulkan SDK; builds both workers into separate
   directories; attaches them to the installer.
3. Move benchmark scripts into `scripts/bench/`; add after-idle and on-battery
   runs; grow the quality set to ≥ 100 clips.

**Gates, in order:**

- (a) The CI-built installer launches on a **clean VM with no GPU driver and no
  VC++ redistributable**; `--list-devices` fails or returns nothing cleanly;
  the app lands on faster-whisper.
- (b) `dumpbin /dependents` shows the CPU worker imports nothing Vulkan and
  neither worker imports VC runtime DLLs.
- (c) On the owner's laptop, `--list-devices` reports both GPUs with types and
  LUIDs; the RTX is chosen; the worker reports the backend that actually ran.
- (d) Worker benchmark matches the spike within 10%; idle, battery and
  residency (VRAM held) numbers recorded; quality within criterion 3.
- (e) Installer size increase measured.

### Phase 0 results (2026-10-16)

Done in commits `3b85897` (worker), `ecce249` (CI + bundling), `c35bbed`
(bench harness). Protocol as implemented: see the header of
`src-tauri/windows/asr-worker/main.cpp`.

- Gate (b) pass: both EXEs import only `kernel32`/`advapi32`; `vulkan-1.dll`
  is **delay-loaded** in the vk worker (a hard import would show a Windows
  "DLL not found" dialog that `SetErrorMode` cannot suppress).
- Gate (c) pass: `--list-devices` (0.2–0.4 s, no model) reports both GPUs with
  type and LUIDs that match DXGI; RTX selected by LUID; backend detection from
  whisper's own log lines; a GPU load that lands on CPU returns a `backend`
  error instead of running silently on CPU. Device mapping pins ggml's list via
  `GGML_VK_VISIBLE_DEVICES` (the Radeon driver reports no PCI id).
- Gate (d) GPU pass: RTX Small 118/187/356/874 ms, RTX Turbo 176/228/340/912 ms,
  iGPU Small 1292/1958/2369/6075 ms (2/5/10/30 s). Quality on 200 clips:
  Small 4.40% (RTX worker) vs 4.35% (faster-whisper CPU); Turbo 2.58% vs
  2.76% (faster-whisper CUDA). VRAM held with Turbo resident: 1,902 MB.
  After 60 s idle the RTX Turbo 10 s clip took 600–730 ms (vs ~400 ms warm);
  2 s clips unaffected.
- Gate (d) CPU worker: 15–30% slower than the spike because `GGML_OPENMP=OFF`.
  **Decision: keep OpenMP off.** The CPU worker is only the fallback for GPU
  users without a faster-whisper model; it is not worth an extra runtime DLL.
  Revisit if whisper.cpp ever becomes the CPU engine.
- Gate (e): vk worker 43.8 MB, cpu worker 2.2 MB (~6 MB compressed).
- Gate (a): no clean VM; CI covers a GPU-less runner, the `dumpbin` check, and
  a real CPU decode with `ggml-tiny.en`. Unverified: machines without the VC++
  redistributable (covered only by `dumpbin`) and without `vulkan-1.dll`.

**New facts that change Phases 1–2:**

- **First GPU use on a machine takes 25–41 s** while the NVIDIA driver compiles
  shaders (cache keyed by EXE name; ggml has no pipeline cache; driver updates
  likely reset it). First-time qualification therefore gets a **120 s load
  deadline**, a cold-cache timeout is "retry", never "fail", and the UI says
  "Getting your graphics card ready (one time, up to a minute)".
- Warm-up only partly absorbs the iGPU's slow first decode (relevant to
  Phase 5).
- Cancel can take ~3 s on CPU (encoder is not interruptible).
- Backend detection parses whisper log text; re-verify whenever the pinned
  whisper.cpp version changes.

### Phase 1: worker integration

1. New transport behind the existing decode interface: writer thread,
   per-phase deadlines, request ids, kill-and-reap, retained audio, insert
   once; outer finalize timeout raised.
2. Error classes and the failure table.
3. Capture/finalization made engine-aware (same path as faster-whisper).
4. GGML catalog entries with real hashes; downloader fixes.
5. Both model families valid; June migration removed.
6. Remove the dormant in-process `whisper-rs` path (`transcribe_with_whisper`,
   `executor.rs`, `backend.rs`, vendored crates) once the transport works,
   rather than carrying both. This also drops bindgen/LIBCLANG and cmake from
   app builds. (Pending owner approval.)

**Gate:** end-to-end microphone dictation on the GPU and CPU workers in a dev
build; killing the worker mid-decode and suspending it both deliver the text
once via the CPU rung.

### Phase 2: selection and product surface

1. CPU eligibility check; DXGI pre-check; `--list-devices` recommendation.
2. Qualification as the worker's first start; caching; transient outcomes;
   driver-change and expiry re-qualification.
3. Upgrade notice for GPU-capable existing users; atomic switch.
4. "GPU acceleration: Auto / Off", "Running on …", step-down notices; polish
   disabled with explanation on non-AVX2 CPUs.

**Gate:** owner walks through a fresh install and an upgrade in the real app on
the RTX laptop, including declined and interrupted upgrades.

### Phase 3: cleanup

1. Delete the dead CUDA add-on (`gpu_runtime.rs` install paths,
   `src/lib/gpuRuntime.ts`) and `Resolve-WhisperFeatureArgs`'s `whisper-cuda`
   auto-enable.

### Phase 4: validation

**Gate:** hardware matrix passes and success criteria 1–5 hold.

### Phase 5: integrated GPUs (gated)

Turn on the GPU path for integrated GPUs where qualification shows a real win.
The baseline must be **faster-whisper CPU** (today's engine), not whisper.cpp
CPU, or the win is overstated. Requires an Intel iGPU in the matrix and a
decision on model format for these users. If `audio_ctx` work lands first,
whisper.cpp may also win on CPU and simplify this phase.

## Testing

No new unit tests (owner rule). End-to-end on real hardware plus the benchmark
harness.

### Benchmark harness

- Production workers, not the spike binary.
- ≥ 100 clips plus the owner's real dictations (kept out of the repo).
- Warm, after-idle, on-battery; latency distributions; raw transcripts with
  polish off.

### Hardware matrix

| Machine | Expected result |
|---|---|
| Owner laptop, RTX 3060 | RTX chosen by identity, Turbo recommended, criterion 1 met |
| Owner laptop, on battery / after 60 s idle | GPU or a "slow, retry next session" outcome; never cached as failed |
| Owner laptop, game holding VRAM | `resource` → CPU for that dictation, GPU again after |
| Owner laptop, GPU acceleration Off | CPU rung, works |
| Owner laptop, NVIDIA disabled in Device Manager | Integrated only → faster-whisper CPU, Small |
| Owner laptop, offline (no network) | Dictation and fallback work; only downloads are unavailable |
| Switching Small ↔ Turbo, deleting models | Worker reloads correctly; fallback model still present |
| AMD discrete GPU (borrow or cloud) | GPU path, Turbo or Small per memory |
| Intel integrated laptop (borrow or cloud) | faster-whisper CPU, unchanged, no crash |
| Clean VM, no GPU driver, no VC++ redist | App launches, faster-whisper CPU |
| CPU without AVX2 (or emulated) | No worker spawned; faster-whisper; polish disabled with explanation |
| Upgrade from v0.6.1, RTX laptop | Keeps dictating; offered GPU model; switches after qualification |
| Upgrade from v0.6.1, no GPU | Nothing changes |
| Sleep/resume, driver update | Lazy respawn; driver update re-qualifies |

### Failure injection

- Kill the GPU worker mid-decode → text delivered once via the CPU rung.
- Suspend the worker (blocks writes and reads) → deadline fires → same.
- Fill GPU memory before load → `resource`, not counted.
- Corrupt GGML file, disk full, cancelled and resumed downloads → no working
  model ever lost.
- Restart the app between GGML download and switch → consistent state.

## Risks

| Risk | Mitigation |
|---|---|
| Vulkan driver bugs (esp. Intel) | Worker isolation; Intel iGPU deferred to Phase 5; Intel Arc qualified like any discrete GPU |
| Static/CRT/OpenMP build details wrong | Phase 0 gates (a) and (b) on a clean VM |
| whisper.cpp 1.9.5 differs in the worker vs the spike | Phase 0 gate (d) |
| Laptop GPU power-down adds latency after idle | Phase 0 idle data; residency decision |
| Held VRAM annoys gamers | Phase 0 residency decision |
| GPU upgraders pay a one-time model download | Explicit offer; faster-whisper keeps working until done |
| Turbo user's GPU fails → CPU Turbo is slow | Notice offers Small |
| Two model formats on disk for upgraders | Models page shows and removes the old one |

## Open questions for the owner

1. **Phase 5 sequencing:** ship discrete GPUs first and integrated GPUs as
   Phase 5 (recommended), or block the release on integrated GPUs too?
2. **Upgrade notice timing:** one-time notice on first launch after the update
   (proposed), or only when the user opens Models / Settings?
3. **Test hardware:** access to an Intel laptop and an AMD discrete GPU, or use
   cloud Windows machines?
4. **Phase 1.6:** OK to remove the dormant in-process `whisper-rs` entirely
   when the worker transport lands? (Both reviewers recommend yes.)
5. **Non-AVX2 CPUs:** OK to disable AI polish there with an explanation
   (its wheel requires AVX2)?

## Later (not this plan)

- `audio_ctx` reduction so whisper.cpp wins on short clips, which could make
  it the CPU engine and let faster-whisper retire.
- Quantized GGML models (smaller downloads).
- AI polish on llama.cpp + Vulkan in a worker, which would let the Python
  runtime stop shipping.
- Faster model families on the same worker and selection infrastructure.

## Changes from v1 (review round 1)

| v1 | v2 | Raised by |
|---|---|---|
| whisper.cpp in-process via whisper-rs | Separate worker executables (vk, cpu) built from upstream | Both (blocker) |
| Crash-marker file | Process isolation with deadlines and restart | Both (blocker) |
| faster-whisper fallback-only | faster-whisper stays the CPU engine; GPU path only where it qualifies | Both |
| Ladder step 3 + fallback-model download for new installs | Gone; GPU users fall back to whisper.cpp CPU on the same GGML file | Fable |
| `--gpu-probe` mode of the main exe | Qualification is the worker's first start | Fable, GPT |
| Probe keyed by app version | Keyed by device, driver, model, worker build, config | Fable |
| Device index assumed | Mapped by identity; worker reports backend actually used | Both |
| Reverse the June model-id migration | Both families valid; atomic switch | GPT |
| Checksum by removing the ggml exemption | Streaming hash; verify before replace | Both |
| "Matched or beat in every pair" | Corrected; non-inferiority tolerance | Both |
| Integrated GPUs in the first release | Phase 5, gated | Data |
| Bundled probe model (proposed) | Rejected by owner | Owner |

## Changes from v2 (review round 2)

| v2 | v3 | Raised by |
|---|---|---|
| Hardware rules ignored the CPU; non-AVX2 + discrete GPU could download GGML with no working engine | CPU eligibility checked first; non-AVX2 never gets workers or GGML; polish disabled there; SSE4.1 is the app minimum | GPT |
| First recommendation from DXGI memory (APU carve-outs misread, old GPUs, Intel Arc excluded) | Model-free `--list-devices` from the Vulkan worker decides; DXGI only as a pre-check | Fable |
| Build details unspecified | Static whisper/ggml, static CRT, OpenMP off, separate dirs, `dumpbin` gate, clean-VM-without-redist gate | GPT, Fable |
| Crash/abort could pop Windows dialogs | `SetErrorMode` + `_set_abort_behavior` in the worker | Fable |
| Deadlines on decode only; synchronous write could block untimed | Writer thread; deadlines for spawn, list, load, transcribe, write; kill-and-reap; retained audio; insert once; outer timeout raised | GPT |
| Failure table punished transient causes (battery, VRAM pressure) for months | Error classes; slow-but-correct and OOM never cached as failures; driver disable expires after 7 days or on update | Both |
| "Chosen by measurement" overstated | Renamed qualification; thresholds on both clips; must beat faster-whisper where installed | GPT |
| Device matched by name/vendor | Persistent identity (vendor, device, UUID) + runtime LUID | GPT |
| Cache invalidated only on whisper.cpp version | Worker build fingerprint and decode config in the key | GPT |
| Integration framed as transport only | Capture/finalization made engine-aware; mic E2E for pauses, short speech, fallback replay | GPT |
| Residency undecided | Default keep-loaded; Phase 0 decides with idle/VRAM data | Fable |
| Sleep/resume missing from failure table | Lazy respawn, not a failure | Fable |
| Remove whisper-rs in Phase 3 | Phase 1, when the transport lands (pending owner OK) | Fable |
| Phase 5 baseline unspecified | Must compare against faster-whisper CPU | Fable |
| Installer size open question | Estimated 20–35 MB; measured in Phase 0; question closed | Fable |
| Testing gaps | Added memory pressure, model switching, offline, clean VM without redist | GPT |
