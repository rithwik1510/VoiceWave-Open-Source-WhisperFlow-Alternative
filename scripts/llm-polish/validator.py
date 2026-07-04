"""Fidelity validator for the on-device LLM polish spike (plan 005).

This is the guardrail that would gate any production polish pass: it decides
whether an LLM rewrite is SAFE to offer, using ONLY the raw transcript and the
rewrite (no per-case ground truth) -- exactly what production has available.

Design goal: catch the trust-destroying failures (a changed name, a dropped
digit, an invented number, a mangled code identifier or URL) while passing
legitimate grammar/filler cleanup. When in doubt, REJECT and fall back to the
deterministic text. False rejects cost nothing (user keeps the good ASR text);
false accepts erode trust irreversibly.

This mirrors the primitives that already exist in the Rust codebase:
  - token overlap  <- asr_integrity_metrics (state.rs:574)
  - protected terms <- stabilize_custom_terms (transcript/mod.rs)
so a GO decision can be ported to Rust with the same semantics.
"""

from __future__ import annotations

import re
from collections import Counter
from dataclasses import dataclass, field

# --- tunable thresholds (tuned on the 15-case set; see spike log) ---
MIN_TOKEN_OVERLAP = 0.60   # fraction of raw content words that must survive
MAX_LENGTH_RATIO = 2.5     # polished word count / raw word count ceiling (anti-runaway)

# --- entity extractors -------------------------------------------------------
# Each returns the set of substrings that MUST appear verbatim in the rewrite.

_RE_EMAIL = re.compile(r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}")
_RE_URL = re.compile(r"(?:https?://|www\.)[^\s,]+", re.IGNORECASE)
_RE_BARE_DOMAIN = re.compile(
    r"\b[A-Za-z0-9\-]+\.(?:com|org|net|io|dev|ai|gov|edu|co|app)\b", re.IGNORECASE
)
_RE_WIN_PATH = re.compile(r"[A-Za-z]:\\[^\s]+")
_RE_UNIX_PATH = re.compile(r"(?<!\w)/[A-Za-z0-9._\-/]+")
_RE_FILENAME = re.compile(
    r"\b[\w\-]+\.(?:rs|py|ts|tsx|js|jsx|json|md|toml|yaml|yml|txt|csv|sh|rb|go|c|cpp|h)\b"
)
# code identifiers: camelCase, snake_case, foo(), a::b, dotted.ident
_RE_CAMEL = re.compile(r"\b[a-z]+[A-Z][A-Za-z0-9]*\b")
_RE_SNAKE = re.compile(r"\b[a-z0-9]+_[a-z0-9_]+\b")
_RE_CALL = re.compile(r"\b[A-Za-z_][\w]*\(\)")
_RE_COLONS = re.compile(r"\b[A-Za-z_][\w]*::[A-Za-z_][\w:]*\b")
# numbers: any digit-bearing run incl. decimals, versions, times, dates, ranges
_RE_NUMBER = re.compile(r"\d[\d,.:/\-]*\d|\d")
# version like v1.2.3 or 1.2.0
_RE_VERSION = re.compile(r"\bv?\d+\.\d+(?:\.\d+)*\b")


@dataclass
class Entities:
    numbers: set = field(default_factory=set)
    urls: set = field(default_factory=set)
    emails: set = field(default_factory=set)
    paths: set = field(default_factory=set)
    code: set = field(default_factory=set)

    def all_items(self):
        for group in (self.numbers, self.urls, self.emails, self.paths, self.code):
            yield from group


def extract_entities(text: str) -> Entities:
    e = Entities()
    e.emails = set(_RE_EMAIL.findall(text))
    e.urls = set(_RE_URL.findall(text)) | set(_RE_BARE_DOMAIN.findall(text))
    e.paths = (
        set(_RE_WIN_PATH.findall(text))
        | set(_RE_UNIX_PATH.findall(text))
        | set(_RE_FILENAME.findall(text))
    )
    e.code = (
        set(_RE_CAMEL.findall(text))
        | set(_RE_SNAKE.findall(text))
        | set(_RE_CALL.findall(text))
        | set(_RE_COLONS.findall(text))
    )
    # numbers: keep versions whole, then standalone number runs, minus those
    # already captured inside emails/urls/paths (avoid double-flagging 192.168)
    consumed = " ".join(e.urls | e.emails | e.paths)
    nums = set(_RE_VERSION.findall(text))
    for m in _RE_NUMBER.findall(text):
        if m not in consumed:
            nums.add(m)
    e.numbers = {n for n in nums if n not in consumed}
    return e


# --- token overlap (mirrors asr_integrity_metrics semantics) -----------------
_RE_WORD = re.compile(r"[A-Za-z0-9']+")
# spoken filler that the polish is *expected* to remove -> excluded from the
# overlap denominator so legitimate cleanup isn't penalized.
_FILLER = {
    "um", "uh", "erm", "like", "you", "know", "i", "mean", "sort", "of", "kind",
    "basically", "actually", "literally", "so", "well", "just", "right",
}


def token_overlap(raw: str, polished: str) -> float:
    """Fraction of raw *content* words (multiset) retained in polished."""
    raw_words = [w.lower() for w in _RE_WORD.findall(raw)]
    content = [w for w in raw_words if w not in _FILLER]
    if not content:
        return 1.0
    raw_counts = Counter(content)
    pol_counts = Counter(w.lower() for w in _RE_WORD.findall(polished))
    kept = sum(min(c, pol_counts.get(w, 0)) for w, c in raw_counts.items())
    return kept / sum(raw_counts.values())


def _norm(s: str) -> str:
    return s.lower().rstrip(".,;:!?)")


@dataclass
class ValidationResult:
    accepted: bool
    overlap: float
    reasons: list

    def label(self) -> str:
        return "ACCEPT" if self.accepted else "REJECT"


def validate(raw: str, polished: str) -> ValidationResult:
    reasons: list[str] = []
    polished = polished.strip()

    if not polished:
        return ValidationResult(False, 0.0, ["empty rewrite"])

    # 1. token overlap floor (guards wholesale rewrite / topic drift)
    overlap = token_overlap(raw, polished)
    if overlap < MIN_TOKEN_OVERLAP:
        reasons.append(f"low token overlap {overlap:.2f} < {MIN_TOKEN_OVERLAP}")

    # 2. length runaway (guards hallucinated additions)
    rw = len(_RE_WORD.findall(raw))
    pw = len(_RE_WORD.findall(polished))
    if rw and pw / rw > MAX_LENGTH_RATIO:
        reasons.append(f"length ratio {pw/rw:.2f} > {MAX_LENGTH_RATIO}")

    # 3. entity preservation: every raw entity must survive verbatim
    raw_ent = extract_entities(raw)
    pol_lower = polished.lower()
    for item in raw_ent.all_items():
        if _norm(item) not in pol_lower:
            reasons.append(f"missing/altered entity: {item!r}")

    # 4. no invented numbers (a number in polished not present in raw = fabricated fact)
    pol_ent = extract_entities(polished)
    raw_nums_norm = {_norm(n) for n in raw_ent.numbers}
    for n in pol_ent.numbers:
        if _norm(n) not in raw_nums_norm:
            reasons.append(f"invented number: {n!r}")

    return ValidationResult(len(reasons) == 0, overlap, reasons)
