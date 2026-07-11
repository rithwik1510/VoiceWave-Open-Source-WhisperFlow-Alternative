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
  {"command": "polish", "id": <n>, "text": "<raw>", "profile": "<p>"} ->
      {"id": <n>, "ok": true, "text": "<polished>"}
      or {"id": <n>, "ok": false, "error": "<why>"}
  {"command": "shutdown"} -> {"ok": true, "shutdown": true} then exit.

`profile` (plan 010) is optional: one of "standard" | "coding" | "writing" |
"casual". Missing/unknown/"standard"/"literal" all take the EXACT pre-010 code
path (byte-identical prompt, same max_tokens, no extra checks). The three new
profiles add few-shot prompts plus a context budget: over-long input returns
{"ok": false, "error": "too_long"} and a truncated generation returns
{"ok": false, "error": "output_truncated"} so the Rust caller falls back to
the deterministic text.
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

# ---------------------------------------------------------------------------
# Profile prompt registry (plan 010, Phase 2).
#
# Design constraints (measured 2026-07-11, scripts/llm-polish/measure_runtime.py,
# CPU @ 4 threads = budget hardware):
#   - llama.cpp reuses the KV prefix between same-profile calls (warm ~1.2s),
#     but switching profiles re-prefills the whole prompt at ~90-110 tok/s.
#     A ~250-token prompt keeps the switch first-call near ~3.5-4s; 400 tokens
#     costs ~6s. So: compact contracts + 3 dense exemplars, NOT long prompts.
#   - LlamaRAMCache was measured and rejected: it cuts switch-back to ~1.4s
#     but costs ~560 MiB RSS per cached prompt state (3.3 GiB for six states).
#
# "standard" (and missing/unknown/"literal") deliberately has NO entry here:
# it takes the exact pre-010 code path with the byte-identical prompt above.
#
# Exemplars are (raw, output) pairs rendered as few-shot chat turns. Fixed
# order, temperature 0, plain-text-only outputs. Each set covers: fillers,
# false starts, identifiers (exact casing), numbers/dates, hedge preservation
# ("maybe" never becomes a directive), questions-as-requests, one short input.
# ---------------------------------------------------------------------------
PROFILE_USER_TEMPLATE = "Dictation:\n{raw}"

PROFILE_PROMPTS = {
    "coding": {
        "system_prompt": (
            "You rewrite voice dictation as terse engineering text: commits, code review notes, technical chat.\n"
            "RULES:\n"
            "1. Copy identifiers, paths, URLs, commands, numbers, versions EXACTLY, casing included - maxRetries never becomes MaxRetries. Never drop quoted words, literal strings, or error text.\n"
            "2. Delete filler (um, uh, like, you know) and false starts. When the speaker corrects themselves (\"no wait\", \"actually no\", \"I mean\"), keep ONLY the last version, never the abandoned one.\n"
            "3. Drop lead-ins like \"I think we should\" before an action, but always keep \"maybe\", \"might\", \"not sure\", and every \"not\".\n"
            "4. Questions stay questions.\n"
            "5. Add nothing, drop no detail. Plain text, no markdown, no preamble."
        ),
        "exemplars": [
            (
                "so um i think we should refactor getUserById to not throw when the user doesnt exist and instead return null",
                "Refactor getUserById to return null instead of throwing when the user doesn't exist.",
            ),
            (
                "uh can you check if the pool size in db_config is still 10 before the march 3rd deploy maybe bump it",
                "Can you check if the pool size in db_config is still 10 before the March 3rd deploy? Maybe bump it.",
            ),
            (
                "use the staging url actually no use the prod url and the smoke test prints qx7fz when it passes",
                "Use the prod url. The smoke test prints qx7fz when it passes.",
            ),
            (
                "send it no wait send it tomorrow morning",
                "Send it tomorrow morning.",
            ),
        ],
    },
    "writing": {
        "system_prompt": (
            "You rewrite voice dictation as polished professional prose for documents and formal email.\n"
            "RULES:\n"
            "1. Complete grammatical sentences, professional register. ALWAYS expand every contraction: \"don't\" becomes \"do not\", \"I'm\" becomes \"I am\", \"it's\" becomes \"it is\", \"let's\" becomes \"let us\".\n"
            "2. Remove chat interjections and slang (lol, haha, omg, yeah, nah, hey, dude) along with filler (um, uh, like, you know) and false starts; keep only the corrected version.\n"
            "3. Keep the speaker's stance: \"maybe\", \"I think\", \"might\", \"probably\" must survive; never turn a suggestion into an order; never drop \"not\".\n"
            "4. Copy names, numbers, dates, emails, URLs, identifiers, and technical terms EXACTLY, casing included. Drop no detail that was spoken.\n"
            "5. Add nothing, summarize nothing. No headings, bullets, or sign-offs."
        ),
        "exemplars": [
            (
                "so um i think we should refactor getUserById to not throw when the user doesnt exist and instead return null",
                "I think we should refactor getUserById so that it returns null rather than throwing an exception when the user does not exist.",
            ),
            (
                "the crash is a race condition in the um in the save path maybe we should not patch it this week im not sure",
                "The crash is a race condition in the save path. Maybe we should not patch it this week; I am not sure.",
            ),
            (
                "lol yeah ill be there um can we push it to 4:30 though its a bit tight",
                "Yes, I will be there. Can we push it to 4:30, though? It is a bit tight.",
            ),
        ],
    },
    "casual": {
        "system_prompt": (
            "You tidy voice dictation into casual chat messages: Slack, texts, DMs.\n"
            "RULES:\n"
            "1. Keep the speaker's voice: casual words stay (\"yeah\", \"nah\", \"hey\", \"lol\", \"gonna\"). ALWAYS use contractions: \"do not\" becomes \"don't\", \"i am\" becomes \"I'm\", \"it is\" becomes \"it's\". Never formalize.\n"
            "2. Delete only pure filler (um, uh) and false starts; keep only the corrected version. Add punctuation and capitals.\n"
            "3. Short and natural. Never headings, bullets, sign-offs, or corporate tone.\n"
            "4. Keep \"maybe\", \"I think\", \"might\", and every \"not\". Never turn a suggestion into an order.\n"
            "5. Copy names, numbers, dates, emails, URLs, identifiers EXACTLY, casing included. Drop no detail.\n"
            "6. Add nothing. Plain text only."
        ),
        "exemplars": [
            (
                "so um i think we should refactor getUserById to not throw when the user doesnt exist and instead return null",
                "I think we should refactor getUserById so it returns null when the user doesn't exist, instead of throwing.",
            ),
            (
                "nah um i am just gonna fix it tonight it will be live by like 9",
                "Nah, I'm just gonna fix it tonight, it'll be live by like 9.",
            ),
            (
                "uh are you free at 3:30 no wait at 4 tomorrow",
                "Are you free at 4 tomorrow?",
            ),
        ],
    },
}

# Context budget (profile paths only; the standard path is untouched).
# n_ctx=2048 must hold: chat-template overhead + system + exemplars + input
# + generated output, with margin.
N_CTX = 2048
CTX_MARGIN = 64
MAX_OUTPUT_TOKENS = 400
# Approximate per-message chat-template overhead (Qwen2.5 im_start/im_end).
_MSG_OVERHEAD_TOKENS = 6


def build_messages(profile, text: str) -> list:
    """Compose the chat messages for a request.

    Any profile without a registry entry (None, "", "standard", "literal",
    unknown strings) yields EXACTLY the pre-010 message list -- byte-identical
    prompt, asserted by scripts/llm-polish/profile_gate.py.
    """
    spec = PROFILE_PROMPTS.get(profile) if isinstance(profile, str) else None
    if spec is None:
        return [
            {"role": "system", "content": SYSTEM_PROMPT},
            {"role": "user", "content": USER_TEMPLATE.format(raw=text)},
        ]
    messages = [{"role": "system", "content": spec["system_prompt"]}]
    for ex_raw, ex_out in spec["exemplars"]:
        messages.append({"role": "user", "content": PROFILE_USER_TEMPLATE.format(raw=ex_raw)})
        messages.append({"role": "assistant", "content": ex_out})
    messages.append({"role": "user", "content": PROFILE_USER_TEMPLATE.format(raw=text)})
    return messages


def _estimate_prompt_tokens(llm, messages: list) -> int:
    total = 0
    for msg in messages:
        total += len(llm.tokenize(msg["content"].encode("utf-8"), add_bos=False))
        total += _MSG_OVERHEAD_TOKENS
    return total + _MSG_OVERHEAD_TOKENS  # trailing assistant header


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

        # CREATE_NO_WINDOW: this worker runs windowless, and on Windows a
        # windowless parent spawning a console exe pops a NEW console window
        # on the user's screen for every call without this flag.
        kwargs = {}
        if sys.platform == "win32":
            kwargs["creationflags"] = subprocess.CREATE_NO_WINDOW
        out = subprocess.check_output(
            ["nvidia-smi", "--query-gpu=memory.free", "--format=csv,noheader,nounits"],
            stderr=subprocess.DEVNULL,
            timeout=5,
            **kwargs,
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

    profile = req.get("profile")
    spec = PROFILE_PROMPTS.get(profile) if isinstance(profile, str) else None
    messages = build_messages(profile, text)

    if spec is None:
        # Pre-010 path, byte-identical behavior for standard/missing/unknown.
        try:
            out = llm.create_chat_completion(
                messages=messages,
                temperature=0.0,
                max_tokens=400,
            )
            polished = clean_output(out["choices"][0]["message"]["content"])
        except Exception as exc:  # noqa: BLE001
            return {"id": request_id, "ok": False, "error": f"polish failed: {exc}"}
        return {"id": request_id, "ok": True, "text": polished}

    # Profile path: enforce the n_ctx budget, cap output tokens, and flag
    # truncated generations so the Rust caller falls back deterministically.
    try:
        input_tokens = len(llm.tokenize(text.encode("utf-8"), add_bos=False))
        prompt_tokens = _estimate_prompt_tokens(llm, messages)
        max_out = min(MAX_OUTPUT_TOKENS, 48 + 2 * input_tokens)
        if prompt_tokens + max_out + CTX_MARGIN > N_CTX:
            return {"id": request_id, "ok": False, "error": "too_long"}
        out = llm.create_chat_completion(
            messages=messages,
            temperature=0.0,
            max_tokens=max_out,
        )
        choice = out["choices"][0]
        if choice.get("finish_reason") == "length":
            return {"id": request_id, "ok": False, "error": "output_truncated"}
        polished = clean_output(choice["message"]["content"])
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
