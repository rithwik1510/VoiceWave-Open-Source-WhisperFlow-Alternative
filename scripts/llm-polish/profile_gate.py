"""Plan 010 Phase 4: profile distinctness + safety + latency gate.

Extends polish_spike.py's approach to the three LLM profiles (coding, writing,
casual). Drives the REAL worker code path: it imports
src-tauri/windows/llm-polish/polish_worker.py, injects a 4-thread CPU Llama,
and calls polish_worker.polish() per request -- so context budgeting,
output-token caps, and truncation flagging are exercised, not simulated.

Checks:
  - unit: standard/missing/unknown/"literal" profiles compose the
    byte-identical pre-010 prompt (asserted against polish_spike's constants).
  - unit: over-budget input returns the structured "too_long" error.
  - safety: spike validator (validator.py) + an identifier-CASING check the
    old harness lacked (maxRetries -> MaxRetries must be caught). Gate:
    0 false-accepts; accepted-output entity+casing preservation = 100%.
  - distinctness: pairwise near-identical rate (normalized token edit
    distance < 0.12, or exact match after case/punct folding) on non-short
    buckets: coding<->writing < 5%, writing<->casual < 5%. Short bucket
    reported separately, excluded from gates.
  - latency: per-profile p50/p95 warm-call latency at 4 threads, plus the
    first-call-after-profile-switch cost, reported separately.

Usage (dev set -- iterate prompts against this ONLY):
  .venv-faster-whisper\\Scripts\\python.exe scripts/llm-polish/profile_gate.py \\
      --model scripts/llm-polish/models/Qwen2.5-1.5B-Instruct-Q4_K_M.gguf

Holdout (run EXACTLY ONCE, after prompts are frozen):
  ... profile_gate.py --model ... --holdout

Writes JSON + markdown to scripts/llm-polish/results/.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import statistics
import sys
import time
from pathlib import Path

os.environ.setdefault("CUDA_VISIBLE_DEVICES", "-1")  # budget hardware = CPU only

_HERE = Path(__file__).resolve().parent
_REPO = _HERE.parent.parent
_WORKER_DIR = _REPO / "src-tauri" / "windows" / "llm-polish"
sys.path.insert(0, str(_HERE))
sys.path.insert(0, str(_WORKER_DIR))

import polish_spike  # noqa: E402  (old prompt constants; no model import at top level)
import polish_worker  # noqa: E402  (the REAL worker module under test)
from validator import validate  # noqa: E402

PROFILES = ["coding", "writing", "casual"]
DIST_PAIRS = [("coding", "writing"), ("writing", "casual")]
NEAR_IDENTICAL_DIST = 0.12
NEAR_IDENTICAL_GATE = 0.05
N_THREADS = 4

# --- identifier-casing check (the hole the 2026-07-10 harness missed) --------
# validator.py compares entities case-insensitively, so maxRetries->MaxRetries
# slipped through. Here: any identifier from the raw text that appears in the
# output with different casing (or not at all, for classes validator.py's
# regexes miss: PascalCase, UPPER_SNAKE) is a violation.
_ID_PATTERNS = [
    re.compile(r"\b[a-z]+[A-Z][A-Za-z0-9]*\b"),              # camelCase
    re.compile(r"\b[A-Z][a-z0-9]+[A-Z][A-Za-z0-9]*\b"),      # PascalCase
    re.compile(r"\b[A-Z][A-Z0-9]*_[A-Z0-9_]+\b"),            # UPPER_SNAKE
    re.compile(r"\b[a-z0-9]+_[a-z0-9_]+\b"),                 # snake_case
    re.compile(r"\b[A-Za-z_][\w]*\(\)"),                     # call()
]


def extract_identifiers(text: str) -> set[str]:
    ids: set[str] = set()
    for pat in _ID_PATTERNS:
        ids |= set(pat.findall(text))
    return ids


# Gibberish/non-word literal protection: a spoken token like "asdfgh" (a
# quoted literal, keysmash, code, or ID) is exactly the content a rewrite must
# not drop, yet no entity regex catches it. Heuristic: alphabetic token,
# len >= 4, containing a consonant run >= 4. Validated against the corpus:
# flags only rare/technical tokens, all of which legit rewrites keep anyway
# (false rejects are safe; false accepts are not). Port to Rust in Phase 3.
_RE_CONS_RUN = re.compile(r"[bcdfghjklmnpqrstvwxz]{4,}")


def gibberish_violations(raw: str, polished: str) -> list[str]:
    low = polished.lower()
    viol = []
    for tok in set(re.findall(r"[a-z]+", raw.lower())):
        if len(tok) >= 4 and _RE_CONS_RUN.search(tok) and tok not in low:
            viol.append(f"dropped literal: {tok}")
    return viol


def casing_violations(raw: str, polished: str) -> list[str]:
    """Identifiers from raw that show up in the output with altered casing,
    or are missing entirely (missing counts: a dropped identifier is unsafe)."""
    viol = []
    low = polished.lower()
    for ident in sorted(extract_identifiers(raw)):
        if ident in polished:
            continue
        if ident.lower() in low:
            viol.append(f"casing: {ident}")
        else:
            viol.append(f"missing: {ident}")
    return viol


# --- ground truth ------------------------------------------------------------
def gt_missing(case: dict, polished: str) -> list[str]:
    # "n't" contractions preserve negation: "don't merge" satisfies
    # must_keep "not" (casual/coding legitimately keep contractions).
    low = polished.lower().replace("n't", " not")
    missing = [m for m in case["must_keep"] if m.lower() not in low]
    missing += [f"exact:{m}" for m in case.get("must_keep_exact", []) if m not in polished]
    return missing


# --- distinctness ------------------------------------------------------------
_TOKEN_RE = re.compile(r"[a-z0-9']+")


def fold_tokens(text: str) -> list[str]:
    return _TOKEN_RE.findall(text.lower())


def token_edit_distance(a: list[str], b: list[str]) -> int:
    if not a:
        return len(b)
    if not b:
        return len(a)
    prev = list(range(len(b) + 1))
    for i, ta in enumerate(a, 1):
        cur = [i]
        for j, tb in enumerate(b, 1):
            cur.append(min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (ta != tb)))
        prev = cur
    return prev[-1]


def normalized_distance(x: str, y: str) -> float:
    ta, tb = fold_tokens(x), fold_tokens(y)
    if not ta and not tb:
        return 0.0
    return token_edit_distance(ta, tb) / max(len(ta), len(tb))


# --- unit checks --------------------------------------------------------------
def run_unit_checks() -> list[str]:
    failures = []
    sample = "so um the meeting is at 3 pm and uh dont be late"
    expected = [
        {"role": "system", "content": polish_spike.SYSTEM_PROMPT},
        {"role": "user", "content": polish_spike.USER_TEMPLATE.format(raw=sample)},
    ]
    for prof in (None, "", "standard", "literal", "definitely_not_a_profile", 123):
        got = polish_worker.build_messages(prof, sample)
        if got != expected:
            failures.append(f"build_messages({prof!r}) is NOT byte-identical to the pre-010 prompt")
    # worker constants must still match the spike (the shipped fidelity contract)
    if polish_worker.SYSTEM_PROMPT != polish_spike.SYSTEM_PROMPT:
        failures.append("worker SYSTEM_PROMPT drifted from polish_spike.SYSTEM_PROMPT")
    if polish_worker.USER_TEMPLATE != polish_spike.USER_TEMPLATE:
        failures.append("worker USER_TEMPLATE drifted from polish_spike.USER_TEMPLATE")
    return failures


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", required=True)
    ap.add_argument("--holdout", action="store_true",
                    help="run the holdout corpus (ONCE, after prompts are frozen)")
    args = ap.parse_args()

    corpus_file = _HERE / ("corpus_holdout.json" if args.holdout else "corpus_dev.json")
    corpus = json.loads(corpus_file.read_text(encoding="utf-8"))["cases"]
    split = "holdout" if args.holdout else "dev"

    unit_failures = run_unit_checks()
    for f in unit_failures:
        print(f"UNIT FAIL: {f}", flush=True)
    if unit_failures:
        return 1
    print(f"unit checks passed (standard prompt byte-identical). corpus={split}, "
          f"{len(corpus)} cases", flush=True)

    from llama_cpp import Llama

    t0 = time.perf_counter()
    llm = Llama(
        model_path=args.model,
        n_ctx=2048,
        n_gpu_layers=0,
        n_threads=N_THREADS,
        n_threads_batch=N_THREADS,
        verbose=False,
    )
    print(f"model loaded in {time.perf_counter() - t0:.1f}s "
          f"(CPU, {N_THREADS} threads)", flush=True)
    polish_worker._LLM = llm  # inject: worker code path, our thread settings

    # per-profile prompt token budget report
    prompt_tokens = {}
    for prof in PROFILES:
        msgs = polish_worker.build_messages(prof, "")
        prompt_tokens[prof] = polish_worker._estimate_prompt_tokens(llm, msgs)
        print(f"prompt budget [{prof}]: ~{prompt_tokens[prof]} tokens "
              f"(system + exemplars + template overhead)", flush=True)

    # run the corpus, profile-major (matches production: consecutive
    # dictations under one profile are warm; the first call is the switch)
    outputs: dict[str, dict[str, dict]] = {p: {} for p in PROFILES}
    switch_cost = {}
    latencies: dict[str, list[float]] = {p: [] for p in PROFILES}
    for prof in PROFILES:
        for i, case in enumerate(corpus):
            t = time.perf_counter()
            resp = polish_worker.polish(
                {"id": f"{prof}-{case['id']}", "text": case["raw"], "profile": prof}
            )
            dt = time.perf_counter() - t
            if i == 0:
                switch_cost[prof] = round(dt, 2)
            else:
                latencies[prof].append(dt)

            rec: dict = {"latency_s": round(dt, 2), "ok": bool(resp.get("ok"))}
            if resp.get("ok"):
                out_text = resp["text"]
                v = validate(case["raw"], out_text)
                casing = casing_violations(case["raw"], out_text) \
                    + gibberish_violations(case["raw"], out_text)
                missing = gt_missing(case, out_text)
                accepted = v.accepted and not casing
                gt_bad = bool(missing)
                rec.update({
                    "text": out_text,
                    "validator_accepted": v.accepted,
                    "validator_reasons": v.reasons,
                    "casing_violations": casing,
                    "accepted": accepted,
                    "gt_missing": missing,
                    "gt_bad": gt_bad,
                    "false_accept": gt_bad and accepted,
                })
            else:
                rec.update({"error": resp.get("error"), "accepted": False,
                            "gt_bad": False, "false_accept": False})
            outputs[prof][case["id"]] = rec
            flag = ""
            if rec.get("false_accept"):
                flag = "  << FALSE ACCEPT"
            elif rec.get("gt_bad"):
                flag = "  (bad output, caught)"
            elif not rec["ok"]:
                flag = f"  (worker error: {rec.get('error')})"
            elif not rec["accepted"]:
                flag = "  (rejected -> fallback)"
            print(f"[{prof:>7}] {case['id']:>4} {rec['latency_s']:>5}s{flag}", flush=True)

    # too_long unit check (profile path budget) -- after runs, needs the model
    too_long_resp = polish_worker.polish(
        {"id": "unit-too-long", "text": "word " * 1600, "profile": "coding"}
    )
    too_long_ok = (too_long_resp.get("ok") is False
                   and too_long_resp.get("error") == "too_long")
    print(f"too_long unit check: {'PASS' if too_long_ok else 'FAIL ' + repr(too_long_resp)}",
          flush=True)

    # --- aggregate -----------------------------------------------------------
    summary: dict = {"split": split, "n_cases": len(corpus),
                     "prompt_tokens": prompt_tokens,
                     "switch_first_call_s": switch_cost,
                     "too_long_check": too_long_ok,
                     "unit_checks": "pass"}

    profile_stats = {}
    total_false_accepts = 0
    for prof in PROFILES:
        recs = outputs[prof]
        ok_recs = [r for r in recs.values() if r["ok"]]
        accepted = [r for r in ok_recs if r["accepted"]]
        fa = sum(1 for r in ok_recs if r["false_accept"])
        total_false_accepts += fa
        acc_preserved = sum(1 for r in accepted if not r["gt_bad"] and not r["casing_violations"])
        lat = latencies[prof]
        profile_stats[prof] = {
            "worker_errors": len(recs) - len(ok_recs),
            "accepted": len(accepted),
            "rejected_fallback": len(ok_recs) - len(accepted),
            "false_accepts": fa,
            "accepted_preservation_pct": round(100.0 * acc_preserved / len(accepted), 1) if accepted else 100.0,
            "latency_warm_p50_s": round(statistics.median(lat), 2) if lat else None,
            "latency_warm_p95_s": round(sorted(lat)[max(0, int(len(lat) * 0.95) - 1)], 2) if lat else None,
        }
    summary["profiles"] = profile_stats
    summary["total_false_accepts"] = total_false_accepts

    # distinctness
    distinct = {}
    for a, b in DIST_PAIRS:
        rows = {"nonshort": {"n": 0, "near": 0, "near_ids": []},
                "short": {"n": 0, "near": 0, "near_ids": []}}
        for case in corpus:
            ra, rb = outputs[a][case["id"]], outputs[b][case["id"]]
            if not (ra["ok"] and rb["ok"]):
                continue
            bucket = "short" if case["bucket"] == "short" else "nonshort"
            d = normalized_distance(ra["text"], rb["text"])
            rows[bucket]["n"] += 1
            if d < NEAR_IDENTICAL_DIST:
                rows[bucket]["near"] += 1
                rows[bucket]["near_ids"].append(f"{case['id']} (d={d:.2f})")
        for bucket in rows.values():
            bucket["rate_pct"] = round(100.0 * bucket["near"] / bucket["n"], 1) if bucket["n"] else 0.0
        distinct[f"{a}<->{b}"] = rows
    summary["distinctness"] = distinct

    # gates
    gates = {
        "false_accepts_zero": total_false_accepts == 0,
        "accepted_preservation_100": all(
            profile_stats[p]["accepted_preservation_pct"] == 100.0 for p in PROFILES
        ),
        "too_long_structured_error": too_long_ok,
    }
    for pair, rows in distinct.items():
        gates[f"distinct_{pair}"] = rows["nonshort"]["rate_pct"] < NEAR_IDENTICAL_GATE * 100
    summary["gates"] = gates
    summary["all_gates_pass"] = all(gates.values())

    print("\n" + "=" * 72, flush=True)
    print(json.dumps(summary, indent=2), flush=True)

    # --- write results (JSON + markdown, existing results/ convention) -------
    outdir = _HERE / "results"
    outdir.mkdir(exist_ok=True)
    stamp = time.strftime("%Y%m%d-%H%M%S")
    base = f"profile-gate-{split}-{stamp}"
    dump = {"summary": summary, "model": Path(args.model).name,
            "n_threads": N_THREADS,
            "cases": [
                {"id": c["id"], "bucket": c["bucket"], "raw": c["raw"],
                 **{p: outputs[p][c["id"]] for p in PROFILES}}
                for c in corpus
            ]}
    (outdir / f"{base}.json").write_text(json.dumps(dump, indent=2), encoding="utf-8")

    md = [f"# Profile gate — {split} set ({stamp})", "",
          f"Model: `{Path(args.model).name}` | CPU, {N_THREADS} threads | {len(corpus)} cases", "",
          "## Gates", ""]
    for g, ok in gates.items():
        md.append(f"- {'PASS' if ok else 'FAIL'} — {g}")
    md += ["", "## Per-profile", "",
           "| profile | prompt tok | switch 1st call | warm p50 | warm p95 | accepted | fallback | false-accepts | preservation |",
           "|---|---|---|---|---|---|---|---|---|"]
    for p in PROFILES:
        s = profile_stats[p]
        md.append(f"| {p} | {prompt_tokens[p]} | {switch_cost[p]}s | {s['latency_warm_p50_s']}s | "
                  f"{s['latency_warm_p95_s']}s | {s['accepted']} | {s['rejected_fallback']} | "
                  f"{s['false_accepts']} | {s['accepted_preservation_pct']}% |")
    md += ["", "## Distinctness (near-identical = norm. token edit dist < 0.12)", "",
           "| pair | non-short rate | short rate (excluded from gate) | near-identical cases |",
           "|---|---|---|---|"]
    for pair, rows in distinct.items():
        md.append(f"| {pair} | {rows['nonshort']['rate_pct']}% ({rows['nonshort']['near']}/{rows['nonshort']['n']}) "
                  f"| {rows['short']['rate_pct']}% ({rows['short']['near']}/{rows['short']['n']}) "
                  f"| {', '.join(rows['nonshort']['near_ids']) or '—'} |")
    md += ["", "## Sample outputs", ""]
    for cid in ("t1" if split == "dev" else "h1",
                "n3" if split == "dev" else "h14",
                "c2" if split == "dev" else "h5"):
        case = next((c for c in corpus if c["id"] == cid), None)
        if not case:
            continue
        md.append(f"**{cid}** raw: `{case['raw']}`")
        for p in PROFILES:
            r = outputs[p][cid]
            md.append(f"- {p}: `{r.get('text', 'ERROR: ' + str(r.get('error')))}`")
        md.append("")
    (outdir / f"{base}.md").write_text("\n".join(md) + "\n", encoding="utf-8")
    print(f"\nWrote {outdir / (base + '.json')} and .md", flush=True)
    return 0 if summary["all_gates_pass"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
