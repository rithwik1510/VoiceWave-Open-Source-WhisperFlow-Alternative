# 021: Dictionary boosting for Parakeet

Status: APPROVED (owner, 2026-10-17). Implementation on `feat/parakeet-boost`, based on `feat/parakeet-engine`.

Review notes (main agent, before handoff):
- Re-casing on any completed match contradicted the bird trap, so it now requires a flipped decision.
- Acronyms are no longer boosted.
- Shared-prefix continuation is called out as the main tuning risk, with traps for it.

## Problem

Parakeet TDT 0.6B v3 (added on `feat/parakeet-engine`) is faster and more accurate than both Whisper models on the owner's RTX 3060. It also returns "" on silence. But it gets the owner's names wrong: Claude, Codex, Parakeet ("Paracrete") and Whisper.

The Whisper models get these right because the dictionary terms go into `initialPrompt`. Parakeet is a transducer: encoder + prediction LSTM + joint network. There is nowhere to feed it text context, so the prompt is silently dropped (`asr_worker.rs` sends `initialPrompt` only for whisper).

The owner wants the dictionary to steer decoding the way the prompt does, not a find-and-replace pass on the output.

## Approach: per-token boosting inside the greedy decoder (shallow fusion with a term trie)

`parakeet_decode` (whisper.cpp 1.9.5, `src/parakeet.cpp` ~:2502) is a plain greedy TDT loop:

1. Run the joint network for frame `t`. This yields raw logits over 8192 word-pieces + blank, then the duration logits.
2. Take the argmax token and the argmax duration.
3. Emit the token if it isn't blank, then advance `t`.

We add a small bonus to the logits of word-pieces that continue spelling a dictionary term, before the argmax. Logits are pre-softmax, so `+b` multiplies that candidate's odds by `e^b` relative to the others.

- **Ambiguous audio:** when "Claude" and "cloud" score close, the term wins.
- **Clear audio:** when the audio clearly says something else, the bonus loses.

This is the same idea as NeMo's phrase boosting for transducers, done greedily.

### Mechanics

**Upstream patch (kept tiny).**
- Add a `logits_filter_callback` (+ user data) to `parakeet_full_params`, mirroring whisper's `logits_filter_callback`.
- Call it in `parakeet_decode` right after `parakeet_joint`, before the greedy argmax, with `(ctx, state, logits, n_vocab_logits, user_data)`.
- Carry it as a patch file in `scripts/asr-worker/`, applied by the build (FetchContent `PATCH_COMMAND` or the build script). The build must fail loudly if the patch doesn't apply.
- All boosting logic lives in our `src-tauri/windows/asr-worker/main.cpp`. The existing `new_token_callback` gives us each emitted token, so the match state advances there.

**Term trie, character-level, case-insensitive.**
- Normalize each term: Unicode-lowercase, collapse whitespace, word boundaries become `▁` (SentencePiece's space marker). So "Claude Code" becomes `▁claude▁code`.
- Match pieces character by character against the trie. Any tokenization of the term matches, with no SentencePiece encoder needed.

**Active matches.**
- Keep a small set of trie nodes, so several terms can be partially matched at once.
- A new match may start only at a word-start piece (one beginning with `▁`).
- On each emitted piece, advance every active node and drop the dead ones.
- Blank tokens don't touch the state.

**Which candidates get the bonus.**
- Only the top-K logits (K≈16) are considered. Boosting only matters among competitive candidates, and this keeps the cost to microseconds per step.
- A candidate gets the bonus if appending its text keeps some active match alive, or starts a new one from the root.
- **Start vs continuation:**
  - **λ_start (≈1.5):** hard to start a term.
  - **λ_cont (≈3.0):** easy to finish one once it has begun.

  Tune both on the test set (below).
- Matching is case-insensitive, so `▁Cl` and `▁cl` are boosted equally. This matters: a case-sensitive start boost would turn "clean" into "Clean" mid-sentence.

**Casing: only where the dictionary changed the model's mind.**
- When a term match completes (and ends on a word boundary or end of text), the worker re-cases that span to the dictionary spelling ("cloud" → boosted → "Claude"). It does this only if the bonus flipped at least one argmax decision inside that match.
- If the model produced the word on its own ("a parakeet is a small bird"), its own spelling and casing stand.
- This is the "only override the model when the dictionary was needed" rule. Partial matches are never touched.
- The Rust post-pass (`stabilize_custom_terms`) keeps running unchanged.

**Protocol.**
- The `transcribe` request gains an optional `boostTerms: string[]`.
- Also an optional `boostStrength` multiplier (default 1.0). A dev env var `VOICEWAVE_PARAKEET_BOOST` overrides it for tuning, and `0` disables boosting.
- Whisper requests are untouched.

### Which dictionary entries become boost terms (Rust side)

Feed the same dictionary terms that currently build the Whisper prompt, then filter:

1. **Skip words the model already knows.**
   - If the term, lowercased or capitalized, as a whole word, is a single vocab piece (`▁these`, `▁These`, `▁small`), the model already knows it as a common word. Boosting it would only push ordinary speech around.
   - The worker does this check because it has the vocab. It reports the skipped terms back in the response, for diagnostics.
   - This rule needs no word-list artifact. It also handles the junk entries in the owner's dictionary ("Things", "These", "Small") automatically.
2. **Skip terms under 3 characters,** acronyms included. A `▁pr` start would tilt every "pre…/pro…" word, and the model already writes "PR" natively.
3. **Cap the count.** At most 200 terms, newest/most-used first, whatever ordering the prompt builder already uses.

The worker builds the trie per request. It's trivial at this size.

## Rejected / deferred

- **Fuzzy find-and-replace after decoding:** guesses from spelling and can't tell "cloud storage" from "Claude Code". Deferred; revisit only if boosting leaves near-miss spellings.
- **Beam search with boosting:** better in theory, but 3–5× the decode cost and a much bigger upstream change. Greedy plus a bonus is enough to start.
- **Re-decoding unsure spans with Whisper + prompt:** two engines per dictation. No.

## Risks

- **Homophones** ("Claude"/"cloud", "Whisper"/"whisper"). Boosting will sometimes pick the term when the user meant the common word. The strength is tuned against explicit trap sentences. Re-casing a completed "whisper" to "Whisper" is accepted: if a word is in the dictionary, the user wants that spelling.
- **Greedy commitment and shared prefixes.** This is the main risk. Once "▁cl" is emitted naturally, an active "claude" match makes λ_cont compete against "ean" in "clean". Too strong a continuation bonus turns "clean" into "claude".
  - λ_cont must stay below typical clear-audio margins.
  - The trap set deliberately includes prefix-sharing words: clean, close, cloud; code, coding, codec; whiskey, whistle; para…
  - λ_start < λ_cont and the top-K limit bound the damage.
- **Carrying an upstream patch.** It is small and isolated, and gets re-checked at each whisper.cpp bump. Worth upstreaming later.
- **Latency.** Top-K × term-trie walk per emitted step. Target: under 2 ms added per dictation. Measure.

## Test plan (non-interactive; nothing played aloud, no desktop input)

**Clips.** Windows SAPI TTS written to WAV files, at least 2 voices, 16 kHz:

- **Positives (~30 sentences):**
  - "ask Claude Code to review the Codex PR"
  - "Parakeet is faster than Whisper"
  - "VoiceWave runs on Vulkan"
  - "open the Next.js config"
  - …
- **Traps (~20 sentences, the common word must stay):**
  - "cloud storage is cheap"
  - "she whispered quietly"
  - "these things are small"
  - "the code compiles"
  - "a parakeet is a small bird"
  - "clean the closet before the cloud sync"
  - "the code uses a video codec"
  - "a whistle and a whiskey"

  The bird sentence must come out as "parakeet", lowercase. Re-casing only happens when the boost flipped a decision, so this sentence checks for over-casing.
- **Dictionary:** a synthetic dictionary with the owner's names plus the known junk entries. Do NOT read the owner's encrypted dictionary.

**Metrics.** For each, run boost off vs on, and sweep λ_start/λ_cont:

| Metric | Gate |
|---|---|
| Term recall on positives | ≥ 90% |
| Trap false boosts | ≤ 1 of 20 |
| WER on the bundled qualification + LibriSpeech clips, with the full dictionary loaded | Unchanged within 0.5 pt |
| Added decode time | ≤ 2 ms median |
| GPU (vk) vs CPU worker | Identical text |

**Owner test (later, slowly).** In the dev app, dictate the same names with the real voice, plus a few everyday sentences containing cloud/whisper/code.
