"""Phase 0 runtime measurements for plan 010 (polish profiles).

Measures, on CPU with 4 threads (approximating budget hardware):
  (a) cold model load time
  (b) warm same-prompt-prefix call (llama.cpp KV prefix reuse)
  (c) prompt-switch call (different system prompt = full re-prefill)
      at system-prompt sizes of ~200 / ~400 / ~700 tokens
  (d) peak RSS with the model loaded
  (e) whether LlamaRAMCache rescues the switch case, and its RSS cost

The (c) numbers set the Phase 2 exemplar budget: pick the largest per-profile
prompt size that keeps a profile-switch first call under ~4s at 4 threads.

Usage:
  .venv-faster-whisper\\Scripts\\python.exe scripts/llm-polish/measure_runtime.py \\
      --model scripts/llm-polish/models/Qwen2.5-1.5B-Instruct-Q4_K_M.gguf

Writes JSON + markdown under scripts/llm-polish/results/.
"""

from __future__ import annotations

import argparse
import json
import time
from pathlib import Path

import psutil

N_THREADS = 4

# Realistic dictation-length user inputs (distinct so no user-side prefix reuse).
INPUT_A = (
    "so um i think we should refactor getUserById to not throw when the user "
    "doesnt exist and instead return null because uh the callers are already "
    "checking for that"
)
INPUT_B = (
    "yeah can you tell marcus that the retry limit in maxRetries is set to 5 "
    "but it should probably be 3 for the beta rollout next thursday"
)
INPUT_C = (
    "okay so the quarterly numbers are up 23 percent and uh we should maybe "
    "schedule the review for march 3rd at 9:30 am eastern just to be safe"
)

# Sentence pool used to synthesize system prompts of a target token size. Two
# distinct pools (X/Y) so "switch" prompts share no prefix at all.
_FILLER_X = (
    "You are a transcription editor that turns raw voice dictation into clean written text. "
    "Fix grammar, punctuation, capitalization, and sentence flow. "
    "Remove filler words and false starts such as um, uh, like, and you know. "
    "Never add information, opinions, or facts that were not spoken by the user. "
    "Reproduce every name, number, date, identifier, and technical term exactly. "
    "Keep the speaker's meaning and intent identical at all times. "
    "Output only the cleaned text with no preamble and no explanation. "
)
_FILLER_Y = (
    "You are an engineering dictation assistant that rewrites spoken notes tersely. "
    "Preserve camelCase and snake_case identifiers character for character always. "
    "Do not invent or respell any API name, file path, or code symbol ever. "
    "Strip hedging filler while keeping the technical claim precisely intact. "
    "Questions must remain questions and uncertainty must remain uncertainty. "
    "Never append commentary, markdown headers, bullet points, or sign-offs. "
    "Return plain text only, formatted as short direct engineering sentences. "
)


def build_prompt(llm, pool: str, target_tokens: int) -> str:
    """Repeat/trim pool sentences until the prompt tokenizes to ~target_tokens."""
    text = pool
    while len(llm.tokenize(text.encode("utf-8"), add_bos=False)) < target_tokens:
        text += pool
    toks = llm.tokenize(text.encode("utf-8"), add_bos=False)
    # binary-ish trim by characters until within +-3 tokens
    lo, hi = 0, len(text)
    while lo < hi:
        mid = (lo + hi) // 2
        if len(llm.tokenize(text[:mid].encode("utf-8"), add_bos=False)) < target_tokens:
            lo = mid + 1
        else:
            hi = mid
    return text[:lo]


def rss_mib() -> float:
    return psutil.Process().memory_info().rss / (1024 * 1024)


def peak_rss_mib() -> float:
    return psutil.Process().memory_info().peak_wset / (1024 * 1024)


def timed_call(llm, system: str, user: str, max_tokens: int = 16) -> float:
    t = time.perf_counter()
    llm.create_chat_completion(
        messages=[
            {"role": "system", "content": system},
            {"role": "user", "content": f"Clean up this dictation:\n\n{user}"},
        ],
        temperature=0.0,
        max_tokens=max_tokens,
    )
    return time.perf_counter() - t


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", required=True)
    args = ap.parse_args()

    from llama_cpp import Llama
    from llama_cpp.llama_cache import LlamaRAMCache

    report: dict = {"n_threads": N_THREADS, "model": Path(args.model).name}

    rss_before = rss_mib()

    # (a) cold load ---------------------------------------------------------
    t0 = time.perf_counter()
    llm = Llama(
        model_path=args.model,
        n_ctx=2048,
        n_gpu_layers=0,          # force CPU; venv build is CUDA-capable
        n_threads=N_THREADS,
        n_threads_batch=N_THREADS,
        verbose=False,
    )
    report["cold_load_s"] = round(time.perf_counter() - t0, 2)
    report["rss_after_load_mib"] = round(rss_mib() - rss_before, 0)
    print(f"cold load: {report['cold_load_s']}s, +{report['rss_after_load_mib']} MiB RSS", flush=True)

    sizes = [200, 400, 700]
    prompts = {}
    for s in sizes:
        pa = build_prompt(llm, _FILLER_X, s)
        pb = build_prompt(llm, _FILLER_Y, s)
        prompts[s] = (pa, pb)
        na = len(llm.tokenize(pa.encode(), add_bos=False))
        nb = len(llm.tokenize(pb.encode(), add_bos=False))
        print(f"built prompts size={s}: A={na} tok, B={nb} tok", flush=True)

    # (b)+(c) warm vs switch, no cache --------------------------------------
    # max_tokens=16 isolates prefill cost; generation adds ~constant tail.
    per_size = {}
    for s in sizes:
        pa, pb = prompts[s]
        first = timed_call(llm, pa, INPUT_A)            # cold-prefix for A
        warm1 = timed_call(llm, pa, INPUT_B)            # same-prefix reuse
        warm2 = timed_call(llm, pa, INPUT_C)            # same-prefix reuse
        switch_to_b = timed_call(llm, pb, INPUT_A)      # full re-prefill
        switch_back = timed_call(llm, pa, INPUT_B)      # full re-prefill again
        per_size[s] = {
            "first_call_s": round(first, 2),
            "warm_same_prefix_s": round(min(warm1, warm2), 2),
            "warm_same_prefix_all": [round(warm1, 2), round(warm2, 2)],
            "switch_s": round(switch_to_b, 2),
            "switch_back_s": round(switch_back, 2),
        }
        print(f"size {s}: first {first:.2f}s | warm {warm1:.2f}/{warm2:.2f}s | "
              f"switch {switch_to_b:.2f}s | switch-back {switch_back:.2f}s", flush=True)
    report["no_cache"] = per_size

    # realistic full call (real output length) on the 400-tok prompt --------
    pa400 = prompts[400][0]
    timed_call(llm, pa400, INPUT_A)  # warm the prefix
    full = timed_call(llm, pa400, INPUT_B, max_tokens=400)
    report["warm_full_output_s_400tok_prompt"] = round(full, 2)
    print(f"warm full-output call (400-tok prompt, real gen): {full:.2f}s", flush=True)

    # (e) LlamaRAMCache ------------------------------------------------------
    rss_pre_cache = rss_mib()
    llm.set_cache(LlamaRAMCache(capacity_bytes=2 << 30))
    cache_res = {}
    for s in sizes:
        pa, pb = prompts[s]
        a1 = timed_call(llm, pa, INPUT_A)   # populate cache for A-prefix
        b1 = timed_call(llm, pb, INPUT_A)   # populate cache for B-prefix
        a2 = timed_call(llm, pa, INPUT_B)   # switch BACK to A: cache hit?
        b2 = timed_call(llm, pb, INPUT_B)   # switch back to B: cache hit?
        cache_res[s] = {
            "prime_a_s": round(a1, 2),
            "prime_b_s": round(b1, 2),
            "switch_back_a_s": round(a2, 2),
            "switch_back_b_s": round(b2, 2),
        }
        print(f"[cache] size {s}: prime {a1:.2f}/{b1:.2f}s | "
              f"switch-back {a2:.2f}/{b2:.2f}s", flush=True)
    report["ram_cache"] = cache_res
    report["ram_cache_rss_cost_mib"] = round(rss_mib() - rss_pre_cache, 0)

    # (d) peak RSS -----------------------------------------------------------
    report["rss_now_mib"] = round(rss_mib(), 0)
    report["peak_rss_mib"] = round(peak_rss_mib(), 0)
    print(f"RSS now {report['rss_now_mib']} MiB | peak {report['peak_rss_mib']} MiB | "
          f"cache cost +{report['ram_cache_rss_cost_mib']} MiB", flush=True)

    outdir = Path(__file__).parent / "results"
    outdir.mkdir(exist_ok=True)
    stamp = time.strftime("%Y%m%d-%H%M%S")
    (outdir / f"measure-runtime-{stamp}.json").write_text(
        json.dumps(report, indent=2), encoding="utf-8"
    )

    md = ["# Polish profile runtime measurements (plan 010 Phase 0)", "",
          f"Model: `{report['model']}` | CPU, {N_THREADS} threads | n_ctx 2048", "",
          f"- Cold load: **{report['cold_load_s']}s**, +{report['rss_after_load_mib']} MiB RSS",
          f"- Peak RSS: **{report['peak_rss_mib']} MiB** (process)",
          f"- Warm full-output call (400-tok prompt): **{report['warm_full_output_s_400tok_prompt']}s**",
          f"- LlamaRAMCache RSS cost: +{report['ram_cache_rss_cost_mib']} MiB", "",
          "| prompt tokens | first call | warm same-prefix | switch (re-prefill) | cache switch-back |",
          "|---|---|---|---|---|"]
    for s in sizes:
        n = per_size[s]
        c = cache_res[s]
        md.append(f"| {s} | {n['first_call_s']}s | {n['warm_same_prefix_s']}s | "
                  f"{n['switch_s']}s | {c['switch_back_a_s']}s |")
    (outdir / f"measure-runtime-{stamp}.md").write_text("\n".join(md) + "\n", encoding="utf-8")
    print(f"\nWrote results to {outdir / f'measure-runtime-{stamp}.json'}", flush=True)


if __name__ == "__main__":
    main()
