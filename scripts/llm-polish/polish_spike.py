"""On-device LLM polish spike harness (plan 005, Phase 1 + Phase 2).

Runs a fixed set of realistic raw-dictation transcripts through a small local
instruction-tuned GGUF and measures:
  Phase 1 (quality) : did it improve readability WITHOUT altering any
                      name / number / code / URL / path / email? (ground truth = per-case `must_keep`)
  Phase 1b (latency): CPU decode p50/p95.
  Phase 2 (validator): would validator.validate() -- which has NO ground truth --
                      correctly reject the fidelity failures and accept the good rewrites?

Usage:
  .venv-faster-whisper\\Scripts\\python.exe scripts/llm-polish/polish_spike.py \\
      --model scripts/llm-polish/models/Qwen2.5-3B-Instruct-Q4_K_M.gguf

Deterministic (temperature 0) so results are reproducible for the gate.
Writes a JSON dump + a markdown table under scripts/llm-polish/results/.
"""

from __future__ import annotations

import argparse
import json
import re
import statistics
import time
from pathlib import Path

from validator import extract_entities, validate

# ---------------------------------------------------------------------------
# The polish prompt -- the fidelity contract lives here.
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
# 15 realistic hard cases. `must_keep` = the entities that MUST survive verbatim
# (ground truth for scoring quality). `note` = what makes the case hard.
# ---------------------------------------------------------------------------
CASES = [
    {
        "id": 1,
        "note": "rambling with heavy filler",
        "raw": "so um i think what we need to do is like basically we need to you know rewrite the onboarding flow because uh a lot of users are kind of dropping off at the second step",
        "must_keep": ["second"],
    },
    {
        "id": 2,
        "note": "unusual person name",
        "raw": "can you email siobhan ng and let her know that the design review is uh pushed to thursday",
        "must_keep": ["siobhan", "ng", "thursday"],
    },
    {
        "id": 3,
        "note": "phone number",
        "raw": "yeah call the vendor back their number is uh 415 555 0147 and ask about the refund",
        "must_keep": ["415", "555", "0147"],
    },
    {
        "id": 4,
        "note": "code identifiers camelCase + snake_case",
        "raw": "so the bug is in the function getUserProfile it calls fetch_user_data before the auth_token is set",
        "must_keep": ["getUserProfile", "fetch_user_data", "auth_token"],
    },
    {
        "id": 5,
        "note": "URL",
        "raw": "um the docs are at https://voicewave.dev/guide/setup just send them that link",
        "must_keep": ["https://voicewave.dev/guide/setup"],
    },
    {
        "id": 6,
        "note": "email address",
        "raw": "forward the invoice to accounts.payable@northwind-labs.com by end of day please",
        "must_keep": ["accounts.payable@northwind-labs.com"],
    },
    {
        "id": 7,
        "note": "money + percentage",
        "raw": "so the quarterly revenue was like 1.4 million which is up 23 percent from last year",
        "must_keep": ["1.4", "23"],
    },
    {
        "id": 8,
        "note": "windows file path",
        "raw": "the log file is at C:\\Users\\rith\\AppData\\voicewave\\diagnostics.json check the last entry",
        "must_keep": ["C:\\Users\\rith\\AppData\\voicewave\\diagnostics.json"],
    },
    {
        "id": 9,
        "note": "technical: API + function call",
        "raw": "um so when you call transcribe() the worker returns a json payload and you have to parse the segments array",
        "must_keep": ["transcribe()", "json", "segments"],
    },
    {
        "id": 10,
        "note": "date + time",
        "raw": "the deploy is scheduled for uh march 3rd at 9:30 am eastern so be online before that",
        "must_keep": ["9:30", "eastern"],
    },
    {
        "id": 11,
        "note": "false starts and self-correction",
        "raw": "i think we should i think we should actually no we should ship the fix on monday not tuesday",
        "must_keep": ["monday", "tuesday"],
    },
    {
        "id": 12,
        "note": "run-on dictated list",
        "raw": "for the trip we need to pack a charger some snacks a water bottle and uh the tickets dont forget the tickets",
        "must_keep": ["charger", "snacks", "water", "tickets"],
    },
    {
        "id": 13,
        "note": "already clean (over-edit / hallucination risk)",
        "raw": "The meeting is confirmed for Friday at noon.",
        "must_keep": ["Friday", "noon"],
    },
    {
        "id": 14,
        "note": "company + acronym + version",
        "raw": "so northwind labs is migrating their api to graphql in version 2.0 which should uh cut latency",
        "must_keep": ["northwind", "graphql", "2.0"],
    },
    {
        "id": 15,
        "note": "name + number + code all at once",
        "raw": "tell marcus that the retry limit in maxRetries is set to 5 but it should be 3 for the beta",
        "must_keep": ["marcus", "maxRetries", "5", "3"],
    },
]


def preserved(polished: str, must_keep: list[str]) -> tuple[bool, list[str]]:
    """Ground-truth check: did every must_keep entity survive verbatim?"""
    low = polished.lower()
    missing = [m for m in must_keep if m.lower() not in low]
    return (len(missing) == 0, missing)


def clean_output(text: str) -> str:
    """Strip preamble/quotes the model may add despite instructions."""
    text = text.strip()
    text = re.sub(r"^(here('|)s|sure|okay|cleaned( up)?( text)?)[:,]?\s*", "", text, flags=re.I)
    if len(text) >= 2 and text[0] in "\"'" and text[-1] in "\"'":
        text = text[1:-1].strip()
    return text


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", required=True)
    ap.add_argument("--n-threads", type=int, default=0, help="0 = llama default")
    ap.add_argument("--n-gpu-layers", type=int, default=0, help="0 = CPU only")
    args = ap.parse_args()

    from llama_cpp import Llama

    model_path = Path(args.model)
    size_mb = model_path.stat().st_size / (1024 * 1024)
    print(f"Loading {model_path.name} ({size_mb:.0f} MB) ...", flush=True)

    t0 = time.time()
    llm = Llama(
        model_path=str(model_path),
        n_ctx=2048,
        n_gpu_layers=args.n_gpu_layers,
        n_threads=(args.n_threads or None),
        verbose=False,
    )
    load_s = time.time() - t0
    print(f"Loaded in {load_s:.1f}s. Running {len(CASES)} cases (temp=0)...\n", flush=True)

    results = []
    latencies = []
    for case in CASES:
        raw = case["raw"]
        t = time.time()
        out = llm.create_chat_completion(
            messages=[
                {"role": "system", "content": SYSTEM_PROMPT},
                {"role": "user", "content": USER_TEMPLATE.format(raw=raw)},
            ],
            temperature=0.0,
            max_tokens=400,
        )
        latency_ms = (time.time() - t) * 1000
        latencies.append(latency_ms)
        polished = clean_output(out["choices"][0]["message"]["content"])

        kept_ok, missing = preserved(polished, case["must_keep"])
        v = validate(raw, polished)

        # ground truth: BAD if the model dropped/altered a must_keep entity
        gt_bad = not kept_ok
        # validator correctness
        if gt_bad and not v.accepted:
            verdict = "caught (TP)"
        elif gt_bad and v.accepted:
            verdict = "FALSE ACCEPT (danger)"
        elif not gt_bad and v.accepted:
            verdict = "accept-good (TN)"
        else:
            verdict = "false-reject (safe)"

        results.append({
            "id": case["id"],
            "note": case["note"],
            "raw": raw,
            "polished": polished,
            "latency_ms": round(latency_ms),
            "must_keep": case["must_keep"],
            "preserved": kept_ok,
            "missing": missing,
            "validator_accepted": v.accepted,
            "validator_overlap": round(v.overlap, 3),
            "validator_reasons": v.reasons,
            "gt_bad": gt_bad,
            "verdict": verdict,
        })

        print(f"[{case['id']:>2}] {case['note']}")
        print(f"   raw : {raw}")
        print(f"   out : {polished}")
        flag = "OK " if kept_ok else "!! ALTERED " + str(missing)
        print(f"   keep: {flag}   validator: {v.label()} (overlap {v.overlap:.2f}) {v.reasons}")
        print(f"   -> {verdict}   [{latency_ms:.0f} ms]\n", flush=True)

    # aggregates
    quality_pass = sum(1 for r in results if r["preserved"])
    false_accepts = sum(1 for r in results if r["verdict"].startswith("FALSE ACCEPT"))
    caught = sum(1 for r in results if r["verdict"].startswith("caught"))
    false_rejects = sum(1 for r in results if r["verdict"].startswith("false-reject"))
    p50 = statistics.median(latencies)
    p95 = sorted(latencies)[int(len(latencies) * 0.95) - 1]

    print("=" * 70)
    print(f"QUALITY  : {quality_pass}/{len(CASES)} preserved all entities")
    print(f"LATENCY  : p50 {p50:.0f} ms | p95 {p95:.0f} ms | load {load_s:.1f}s (CPU)")
    print(f"VALIDATOR: caught {caught} bad | {false_accepts} FALSE ACCEPTS | {false_rejects} false-rejects (safe)")
    print("=" * 70)

    outdir = Path("scripts/llm-polish/results")
    outdir.mkdir(parents=True, exist_ok=True)
    stamp = time.strftime("%Y%m%d-%H%M%S")
    dump = {
        "model": model_path.name,
        "model_size_mb": round(size_mb),
        "n_gpu_layers": args.n_gpu_layers,
        "load_s": round(load_s, 1),
        "quality_pass": quality_pass,
        "n_cases": len(CASES),
        "latency_p50_ms": round(p50),
        "latency_p95_ms": round(p95),
        "false_accepts": false_accepts,
        "caught": caught,
        "false_rejects": false_rejects,
        "results": results,
    }
    (outdir / f"run-{stamp}.json").write_text(json.dumps(dump, indent=2), encoding="utf-8")
    print(f"\nWrote {outdir / f'run-{stamp}.json'}")


if __name__ == "__main__":
    main()
