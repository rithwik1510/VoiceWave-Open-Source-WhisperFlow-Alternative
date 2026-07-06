# Plan 007: Stats dashboard — "Your Voice, in numbers"

Planned 2026-07-06 after competitor research (Wispr Flow Usage tab) and a data
audit of the local diagnostics store. Goal: a Stats tab interesting enough to
be a habit hook, with numbers the user can trust. Execution is phased; the
maintainer chose **hero tier first**.

## Research summary (Wispr Flow Usage tab)

Animated semicircular WPM gauge + percentile badge vs keyboard typists; total
words with month-over-month comparison badge; "corrections by Flow" count;
per-app usage bars with hover intensity labels; streak heatmap where
consecutive days glow; team leaderboards (cloud). Psychology: skill score
(WPM), loss aversion (streak), progress (totals), identity (top apps).

Our edge: diagnostics already records ~40 fields per dictation locally
(word counts raw+final, audio duration, latency, insertion target class,
model confidence, mic volume). Richer stats than a cloud product, computed
entirely on-device — and the page should say so.

## The dashboard

### Tier 1 — HERO (build first)
1. **Time saved** — headline hero card (revolving ring, count-up):
   `time_saved = words/40wpm_typing − speaking_time`. Shown for this month +
   all-time. The number that justifies the app.
2. **Speaking speed gauge** — animated semicircular gradient arc, WPM as a
   whole number, "N× faster than typing" kicker, personal-best chip.
   `wpm = final_words / (audio_duration_min)` per dictation, aggregated as a
   duration-weighted average (NOT mean of per-dictation WPMs).
3. **Words dictated** — today / this week / all-time, with a
   month-vs-last-month badge.

### Tier 2 — HABIT
4. Streak + 12-week activity heatmap (blue ramp, glow on current streak,
   hover = words + top app that day).
5. Personal records & milestone chips (longest dictation, biggest day,
   fastest WPM; particle burst on unlock — onboarding celebration language).

### Tier 3 — INSIGHT (our differentiators)
6. **Clarity score** — model confidence (fw_avg_logprob / no_speech_prob)
   mapped to a friendly 0-100 "how clearly VoiceWave hears you" + 7-day
   sparkline. Doubles as early warning for the recurring Windows mic-volume
   drops.
7. **Words cleaned up** — raw−final word count delta ("412 ums never made it
   to the page").
8. **Where you dictate** — top insertion target classes as gradient bar rows.

Rejected: leaderboards (needs cloud/accounts), content-category breakdowns
(requires reading text; app-class gives the insight without it).

## Data accuracy contract (tier 1)

- Source of truth: a new always-on **aggregate store** (per-day rollups:
  dictation count, final words, audio ms, per-app-class counts, best-WPM,
  latency sum) fed at the post-dictation seam. Pure numbers, no transcript
  text — privacy story unchanged. Plain JSON (nothing sensitive to encrypt).
- **Backfill once** from the existing diagnostics records (up to 5000) so the
  dashboard opens full of real history.
- Only count `success == true` mic dictations with `final_word_count > 0`.
- WPM uses audio_duration_ms (actual speaking time), duration-weighted;
  ignore dictations under 2s (WPM on one word is noise).
- Typing baseline 40 WPM (cited average) — constant named in code, shown in
  the UI footnote so the math is honest.
- Diagnostics store stays capped/opt-in-for-export as today; aggregates are
  what survive beyond 5000 records.

## Placement

Dedicated **Stats** nav tab + a one-line Home teaser ("2,140 words this
week →").

## Status

- Tier 1: DONE (2026-07-06) — `stats` module (per-day rollups, plain JSON,
  always-on, in-memory fallback), one-time diagnostics backfill,
  `get_stats_summary` command, Stats nav tab (time-saved hero with ring +
  count-up, WPM gauge, words panel with month-over-month badge), live refresh
  on `voicewave://history-updated`. Accuracy verified against the live
  diagnostics store: Rust gates reproduce the audited numbers exactly
  (1,959 dictations / 58,316 words / 132.6 WPM / 16.97 h saved). Note: stats
  recording is unconditional (aggregates only) — diagnostics opt-in gates only
  the detailed per-utterance records; a dictation counts even if insertion
  failed (verified zero such records in live data anyway).
- Tier 2: TODO
- Tier 3: TODO
