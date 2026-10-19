# 021: Dictionary boosting for Parakeet

Status: IMPLEMENTED 2026-10-19 on `feat/parakeet-boost` (based on `feat/parakeet-engine`). Approved by the owner 2026-10-17. See "Deviations" and "Results" at the end. Owner voice test pending.

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

## Deviations (implementation, 2026-10-19)

Found in the code or on the test set; the conservative choice was taken each time.

- **"Skip words the model already knows" catches almost nothing.** Parakeet v3's 8192-piece vocab is multilingual: `▁these` is a piece, but `▁small`, `▁things`, `▁cloud` and `▁code` are not. The rule is kept as written. Junk entries are contained by the guards below instead; the "these things are small" trap never changed.
- **All acronyms are skipped**, not only those under 3 characters: a term with no lowercase letter (API, GPU, NASA) is never boosted.
- **Blank is never boosted against.** The bonus only reorders word-pieces at steps where the model already chose to emit a token, so boosting can't insert a word into silence or a pause.
- **Start bonus needs a piece with 2+ letters** (`▁cl`, not `▁T`). The 1-letter start bonus turned "tempest" into "Tempest" and "8GB" into "8 GB". A 1-letter piece the model chose itself still opens a match, because names often start that way (`▁V ul kan`).
- **Continuations beat starts.** In a step where some candidate continues an open match, start bonuses are withheld. Otherwise `▁Co` (start of "Codex") beat `co` (continuing "goatcounter").
- **No re-casing flips.** A candidate gets no bonus if it spells the same letters as the model's choice in a different case, in either direction (`▁T` over `▁tem`, `▁cl` over `▁C`).
- **Candidates.** Only pieces within the largest bonus of the top logit can win, so the candidate set is those pieces, capped at K = 16.
- **λ_cont = 7, not ≈3.** On TTS the logit margins are large (often 5–10), so 3 changed almost nothing. λ_start stays 1.5. Both are the highest values that changed no trap sentence (see Results).
- **Skipped terms are reported as counts only** (owner rule: no dictionary terms in diagnostics). The reply carries `"boost":{"terms","skipped","completed","flipped","ms"}`; Rust logs it at debug level.
- **Env var.** `VOICEWAVE_PARAKEET_BOOST` also accepts `start,cont` for tuning.
- **Partial matches.** They are never re-cased, but a continuation bonus can change a partial spelling ("cloud" → "claud" at λ_cont ≥ 9). That is the limit on λ_cont.
- **Superseded risk note.** The Risks line accepting "whisper" → "Whisper" re-casing is superseded by the review note. A completed match is re-cased only if the bonus flipped a decision inside it, and Rust's `stabilize_custom_terms` leaves plain capitalized words alone. So a common-word term the model spells on its own ("parakeet", "whisper", "convex") stays lowercase mid-sentence.
- **avgLogProb.** For Parakeet it is computed from the boosted logits, so it reads slightly higher on boosted tokens.

## Results (2026-10-19, worker CLI, q8_0 model, RTX 3060 via Vulkan index 1, and CPU)

Test set (scratch only, nothing played aloud):
- SAPI David + Zira, 16 kHz.
- 30 positives (80 term slots), each spoken the way a person says the term ("Vulcan", "Versel", "goat counter").
- 20 traps.
- A 10 dB white-noise copy of every clip.
- Synthetic dictionaries: 34 terms (owner names, Things/These/Small, PR/API/GPU/CPU) and a 200-term stress list.

Recall is scored the way the app shows it. Terms with distinct casing (multi-word, inner capitals, dots) are compared case-insensitively, since the Rust post-pass fixes them; plain capitalized words are compared case-sensitively. The "ci" column ignores case throughout.

| Default 1.5 / 7, 34 terms | Recall clean (ci) | Recall noisy (ci) | Trap sentences changed | WER, 6 clips / 358 words |
|---|---|---|---|---|
| vk, boost off | 58.8% (71.2%) | 50.0% (61.3%) | — | 3.07% |
| vk, boost on | 73.8% (86.2%) | 71.2% (82.5%) | 0/20 clean, 0/20 noisy | 3.07% (texts identical) |
| cpu, boost off | 60.0% (72.5%) | 47.5% (61.3%) | — | 3.07% |
| cpu, boost on | 73.8% (86.2%) | 67.5% (81.2%) | 0/20, 0/20 | 3.07% |
| vk, 200 terms, on | 73.8% (86.2%) | 68.8% (80.0%) | 0/20, 0/20 | 3.07% |
| cpu, 200 terms, on | 73.8% (86.2%) | 65.0% (78.8%) | 0/20, 0/20 | 3.07% |

- **Sweep (vk, 34 terms).** Traps first change at 3/9 ("codec" → "Codex"); at 3/10 and above, "cloud" → "claud(s)", "whiskey" → "Whisper", "tail" → "Tailwind". With 200 terms under noise, a start bonus of 3 changed 4 trap sentences ("sink" → "s Ink", "wagged" → "ragged"). 1.5/7 is the only tested point with 0 changes on all four sets.
- **Added decode time.** Measured inside the worker: 0.09 ms median per clip (vk, 34 terms), 0.11 ms with 200 terms. It grows with length: 1.9–2.3 ms on a 60 s clip. Gate met.
- **vk vs cpu.** Not identical, and not because of boosting: with boost off, 7 of 206 clips already differ (backend numerics on borderline words, e.g. "next js" vs "next.js"). With boost on, 8 differ, because a borderline boost can land on one backend and not the other (vk "Vercel", cpu "Versal").
- **Gates.** Traps (≤1/20): met. WER (±0.5 pt): met. Latency (≤2 ms median): met. Recall (≥90%): **not met**, 86% case-insensitive and 74% as scored. What is left: TTS mispronunciations the model hears confidently ("archetype", "Tory"), split forms ("telemetry deck"), and model-spelled lowercase common words (by design, above). vk = cpu: not met, for the pre-existing backend reason.
