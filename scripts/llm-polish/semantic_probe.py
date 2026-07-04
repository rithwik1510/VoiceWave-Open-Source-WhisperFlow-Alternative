"""Semantic-drift probe for the LLM polish spike (plan 005 follow-up).

The Phase-1 validator catches ENTITY alterations and gross rewrites, but NOT
meaning drift where words are mostly preserved but sense changes -- above all
NEGATION flips ("I don't think we should" -> "I think we should"). This probe
stresses that surface: negation, modality, conditionals. For each case it prints
the model output, the entity/overlap validator verdict, and a NEW polarity check
(negation-marker count preserved after contraction expansion). Goal: (a)
reproduce the drift the user saw, (b) prove a polarity guard catches it.

Usage:
  .venv-faster-whisper\\Scripts\\python.exe scripts/llm-polish/semantic_probe.py \\
      --model scripts/llm-polish/models/Qwen2.5-1.5B-Instruct-Q4_K_M.gguf
"""

from __future__ import annotations

import argparse
import re
import time

from validator import validate

# --- polarity / negation preservation check ---------------------------------
# Contractions expanded so "don't" == "do not" doesn't false-flag.
_CONTRACTIONS = {
    "don't": "do not", "doesn't": "does not", "didn't": "did not",
    "won't": "will not", "wouldn't": "would not", "can't": "can not",
    "cannot": "can not", "couldn't": "could not", "shouldn't": "should not",
    "isn't": "is not", "aren't": "are not", "wasn't": "was not",
    "weren't": "were not", "haven't": "have not", "hasn't": "has not",
    "hadn't": "had not", "mustn't": "must not", "needn't": "need not",
    "ain't": "is not",
}
# Core negation markers (after expansion). "not" absorbs every n't.
_NEG = {
    "not", "no", "never", "none", "nobody", "nothing", "nowhere",
    "neither", "nor", "without", "cannot",
}
_MODALS = {"might", "may", "could", "should", "must", "will", "would", "can", "shall"}


def _expand(text: str) -> str:
    t = text.lower()
    for c, full in _CONTRACTIONS.items():
        t = t.replace(c, full)
    return t


def _count(text: str, vocab: set) -> int:
    words = re.findall(r"[a-z']+", _expand(text))
    return sum(1 for w in words if w in vocab)


def polarity_ok(raw: str, polished: str) -> tuple[bool, str]:
    rn, pn = _count(raw, _NEG), _count(polished, _NEG)
    if rn != pn:
        return False, f"negation count {rn}->{pn}"
    rm, pm = _count(raw, _MODALS), _count(polished, _MODALS)
    # modal changes are softer (spoken "gonna"->"will" is fine); only flag a DROP
    if pm < rm:
        return False, f"modal count {rm}->{pm} (dropped)"
    return True, ""


SYSTEM_PROMPT = None  # loaded from polish_spike to stay identical


CASES = [
    "i don't think we should ship this on friday",
    "we can't merge until the tests pass",
    "make sure you never push directly to main",
    "i'm not sure the api is ready yet",
    "we should probably wait unless marketing says otherwise",
    "the feature works fine without the cache",
    "don't forget to update the changelog before you tag the release",
    "i think we might be able to finish by monday but i'm not certain",
    "it's not impossible but it's pretty unlikely to land this week",
    "we don't need to refactor everything right now",
    "she said she wouldn't be able to join the standup",
    "there's no way we can hit that deadline without more people",
    "keep the old flow until the new one is actually stable",
    "i'd rather not change the schema this late",
    "that approach didn't work the last time we tried it",
    "we can ship it but only to beta users for now",
    "nobody has reviewed the security changes yet",
    "unless something breaks overnight we're good to go",
    "let's not merge this until after the demo",
    "i wouldn't say it's ready but it's close",
]


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", required=True)
    args = ap.parse_args()

    # reuse the exact production prompt
    import importlib.util
    spec = importlib.util.spec_from_file_location(
        "polish_spike", "scripts/llm-polish/polish_spike.py")
    ps = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(ps)

    from llama_cpp import Llama
    llm = Llama(model_path=args.model, n_ctx=2048, n_gpu_layers=0, verbose=False)

    drift = 0
    caught_by_polarity = 0
    passed_current = 0
    for i, raw in enumerate(CASES, 1):
        out = llm.create_chat_completion(
            messages=[
                {"role": "system", "content": ps.SYSTEM_PROMPT},
                {"role": "user", "content": ps.USER_TEMPLATE.format(raw=raw)},
            ],
            temperature=0.0, max_tokens=300,
        )
        polished = ps.clean_output(out["choices"][0]["message"]["content"])

        v = validate(raw, polished)                 # current validator
        pol_ok, pol_reason = polarity_ok(raw, polished)
        meaning_drift = not pol_ok                   # proxy: polarity change == drift

        if meaning_drift:
            drift += 1
        if v.accepted:
            passed_current += 1
            if meaning_drift:
                # current validator let a drifted rewrite through; polarity would catch it
                caught_by_polarity += 1

        flag = "DRIFT" if meaning_drift else "ok"
        print(f"[{i:>2}] {flag}")
        print(f"   raw : {raw}")
        print(f"   out : {polished}")
        print(f"   current-validator: {v.label()} {v.reasons}")
        print(f"   polarity-guard   : {'OK' if pol_ok else 'REJECT ('+pol_reason+')'}\n")

    print("=" * 70)
    print(f"Cases: {len(CASES)}")
    print(f"Meaning drift (polarity changed): {drift}")
    print(f"Passed the CURRENT validator: {passed_current}")
    print(f"  of those, drifted (current validator MISSED): {caught_by_polarity}")
    print(f"Polarity guard would have caught those {caught_by_polarity}.")
    print("=" * 70)


if __name__ == "__main__":
    main()
