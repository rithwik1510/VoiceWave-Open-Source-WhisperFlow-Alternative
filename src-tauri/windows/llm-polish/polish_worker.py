"""Thin CPU on-device LLM polish worker (plan 005, Phase 3).

Mirrors the stdin/stdout JSON-line protocol of the faster-whisper worker
(`src-tauri/windows/faster-whisper/worker.py`): read one JSON line from
stdin, dispatch on `command`, print one JSON response line with `flush=True`.

This is the off-by-default "polish" path. The Rust spawner sets
`VOICEWAVE_POLISH_MODEL_PATH` to a local GGUF. The model is lazy-loaded on
the first `polish` command and cached for the process lifetime. Nothing here
touches the network; inference runs on the local GPU when a CUDA-capable build
and enough free VRAM are available (see `_resolve_gpu_layers`), else on the CPU.

Commands:
  {"command": "polish", "id": <n>, "text": "<raw>"} ->
      {"id": <n>, "ok": true, "text": "<polished>"}
      or {"id": <n>, "ok": false, "error": "<why>"}
  {"command": "shutdown"} -> {"ok": true, "shutdown": true} then exit.
"""

from __future__ import annotations

import json
import os
import re
import sys
import traceback
from pathlib import Path

# ---------------------------------------------------------------------------
# The polish prompt -- the fidelity contract. Copied VERBATIM from
# scripts/llm-polish/polish_spike.py (SYSTEM_PROMPT). Do not edit here without
# editing there; the spike measured fidelity against exactly this text.
# ---------------------------------------------------------------------------
SYSTEM_PROMPT = """You are a transcription editor. You turn raw voice dictation into clean, polished written text.

ABSOLUTE RULES:
1. Fix grammar, punctuation, capitalization, and sentence flow so it reads like well-written text.
2. Remove filler and false starts: "um", "uh", "like", "you know", "I mean", repeated words, and self-corrections (keep only the corrected version).
3. NEVER add information, opinions, or facts that were not spoken.
4. Reproduce EXACTLY, character for character, every: person/place/company name, number, date, time, money amount, percentage, email address, URL, file path, code identifier (function/variable/type names), version number, and technical term. Do not "fix" or reformat them.
5. Keep the speaker's meaning and intent identical. Do not paraphrase beyond what grammar cleanup requires. Do not summarize.
6. Output ONLY the cleaned text. No preamble, no quotation marks around it, no notes, no explanation."""

USER_TEMPLATE = "Clean up this dictation:\n\n{raw}"

# Lazily-populated singleton Llama handle (loaded on first `polish`).
_LLM = None

# Minimum FREE VRAM (MiB) required to run the model on the GPU. The 1.5B Q4
# model needs ~1.4 GB resident; this margin also leaves room for the ASR model
# that stays warm on the same card. Below this -> fall back to CPU. Tunable via
# VOICEWAVE_POLISH_MIN_FREE_VRAM_MIB. (Small-card refinement is a productionization item.)
POLISH_MIN_FREE_VRAM_MIB = int(os.getenv("VOICEWAVE_POLISH_MIN_FREE_VRAM_MIB", "1800"))


def _env_flag(name: str, default: bool = False) -> bool:
    raw = os.getenv(name)
    if raw is None:
        return default
    return raw.strip().lower() in ("1", "true", "yes", "on")


def _gpu_free_mib() -> int | None:
    """Free VRAM on GPU 0 via nvidia-smi, or None if it can't be determined."""
    try:
        import subprocess

        out = subprocess.check_output(
            ["nvidia-smi", "--query-gpu=memory.free", "--format=csv,noheader,nounits"],
            stderr=subprocess.DEVNULL,
            timeout=5,
        )
        return int(out.decode().strip().splitlines()[0].strip())
    except Exception:  # noqa: BLE001
        return None


def _resolve_gpu_layers() -> int:
    """Mirror the ASR worker's GPU gating: honor env overrides, otherwise use
    the GPU only if this is a CUDA-capable llama build AND there is enough free
    VRAM to co-exist with the warm ASR model. Returns -1 (offload all) or 0 (CPU)."""
    if _env_flag("VOICEWAVE_POLISH_FORCE_CPU"):
        return 0
    try:
        from llama_cpp import llama_supports_gpu_offload

        gpu_build = bool(llama_supports_gpu_offload())
    except Exception:  # noqa: BLE001
        gpu_build = False
    if not gpu_build:
        return 0  # CPU-only llama build -> nothing to offload
    if _env_flag("VOICEWAVE_POLISH_FORCE_GPU"):
        return -1
    free = _gpu_free_mib()
    if free is None or free < POLISH_MIN_FREE_VRAM_MIB:
        return 0  # not enough headroom next to whisper -> CPU
    return -1  # offload all layers to GPU


def clean_output(text: str) -> str:
    """Strip preamble/quotes the model may add despite instructions.

    Mirrors clean_output in scripts/llm-polish/polish_spike.py.
    """
    text = text.strip()
    text = re.sub(r"^(here('|)s|sure|okay|cleaned( up)?( text)?)[:,]?\s*", "", text, flags=re.I)
    if len(text) >= 2 and text[0] in "\"'" and text[-1] in "\"'":
        text = text[1:-1].strip()
    return text


def _resolve_model_path() -> Path | None:
    raw = os.getenv("VOICEWAVE_POLISH_MODEL_PATH")
    if not raw:
        return None
    candidate = Path(raw.strip())
    if not candidate.exists():
        return None
    return candidate


def _load_model():
    """Lazy-load the GGUF once and cache it. Returns the Llama handle or None."""
    global _LLM
    if _LLM is not None:
        return _LLM
    model_path = _resolve_model_path()
    if model_path is None:
        return None
    from llama_cpp import Llama

    _LLM = Llama(
        model_path=str(model_path),
        n_ctx=2048,
        n_gpu_layers=_resolve_gpu_layers(),
        verbose=False,
    )
    return _LLM


def polish(req: dict) -> dict:
    request_id = req.get("id")
    text = req.get("text")
    if not isinstance(text, str) or not text.strip():
        return {"id": request_id, "ok": False, "error": "empty text"}

    llm = _load_model()
    if llm is None:
        return {"id": request_id, "ok": False, "error": "model not found"}

    try:
        out = llm.create_chat_completion(
            messages=[
                {"role": "system", "content": SYSTEM_PROMPT},
                {"role": "user", "content": USER_TEMPLATE.format(raw=text)},
            ],
            temperature=0.0,
            max_tokens=400,
        )
        polished = clean_output(out["choices"][0]["message"]["content"])
    except Exception as exc:  # noqa: BLE001
        return {"id": request_id, "ok": False, "error": f"polish failed: {exc}"}

    return {"id": request_id, "ok": True, "text": polished}


def main() -> int:
    print(json.dumps({"ready": True}), flush=True)
    for line in sys.stdin:
        raw = line.strip()
        if not raw:
            continue
        try:
            req = json.loads(raw)
            command = req.get("command", "polish")
            if command == "shutdown":
                print(json.dumps({"ok": True, "shutdown": True}), flush=True)
                return 0
            if command != "polish":
                print(
                    json.dumps(
                        {
                            "id": req.get("id"),
                            "ok": False,
                            "error": f"Unsupported command: {command}",
                        }
                    ),
                    flush=True,
                )
                continue
            print(json.dumps(polish(req)), flush=True)
        except Exception as exc:  # noqa: BLE001
            print(
                json.dumps(
                    {
                        "id": None,
                        "ok": False,
                        "error": f"Worker exception: {exc}",
                        "traceback": traceback.format_exc(),
                    }
                ),
                flush=True,
            )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
