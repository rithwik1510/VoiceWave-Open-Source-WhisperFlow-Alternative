# Plan 013: Stats depth, accuracy, and the GitHub-style streak heatmap

> **Reviewer**: This is a draft plan for review, not for execution yet. It was
> written to fill the remaining tiers of `plans/007-stats-dashboard.md` (Tier 2
> and Tier 3, both currently TODO) and to close one accuracy gap the maintainer
> and I identified in the stats data flow. It is intentionally non-destructive —
> it only adds fields, one new command, and new UI. Nothing existing is removed
> or reworked.

## Status

- **Priority**: P2 (accuracy fix is cheap and important; the heatmap and
  insight panels are the plan-007 Tier 2/3 that are still TODO)
- **Effort**: M (mostly additive backend fields + a new frontend panel; no
  schema migration risk because stores are resilient rollups)
- **Risk**: LOW-MED (stats are anonymous rollups; adding fields cannot corrupt
  existing data; biggest risk is UX clutter — mitigated by panel layout)
- **Depends on**: `plans/007-stats-dashboard.md` (Tier 1 DONE) — this plan is
  "the rest of 007"
- **Category**: correctness + feature
- **Planned at**: 2026-08, working tree at v0.5.9 (`main`)

## Why this matters

The Stats tab currently shows the Tier 1 hero (time saved, WPM gauge, words).
Three things are missing that materially improve it:

1. **Accuracy gap**: a dictation counts toward "words dictated / time saved"
   even when the text fails to insert. `state.rs:4693` calls
   `record_dictation` unconditionally, not gated on `insertion_success`. The
   field is already in scope at that exact line. (Plan 007's own note says live
   data currently has zero failed-insertion cases, so the *impact* today is near
   nil — but the gate is cheap, correct, and future-proofs the numbers.)
2. **The streak heatmap** — the single most engaging visual in plan-007's
   Tier 2 ("HABIT"): consecutive active days glow, hover shows detail. GitHub
   uses this to drive habit. We already record a per-day `DayStats` bucket for
   every active day, so the data is there — we just never expose it.
3. **Insight panels from Tier 2/3** that use data we *already collect but never
   render*:
   - "Where you dictate" (`DayStats.app_classes` is collected every dictation
     but never shown).
   - "Words cleaned up" (`raw_words` vs `final_words` delta is collected but
     never shown).
   - "Clarity score" (needs `fw_avg_logprob` / `fw_no_speech_prob`, which today
     live only in the opt-in diagnostics store — needs a small aggregate carry).

So this plan is "finish the job plan 007 started, and make every number honest."

---

## Part A — Accuracy: gate stats on successful insertion

Backend only, tiny.

### A1. Gate `record_dictation` on `insertion_success`

In `src-tauri/src/state.rs` near line 4693, wrap the always-on stats call so a
failed insert does not inflate "words dictated / time saved":

```rust
// Always-on aggregate stats (counts and durations only, never text).
// Only successful insertions feed the "words dictated / time saved" numbers
// so a decode that never landed does not inflate them.
if insertion_success {
    if let Err(err) = self.stats_manager.lock().await.record_dictation(
        now_utc_ms(),
        asr_final_word_count,
        asr_raw_word_count,
        audio_duration_ms,
        Some(insertion_target_class.as_str()),
    ) {
        eprintln!("stats record failed: {err}");
    }
}
```

### A2. Mirror the gate in the backfill

`backfill_from_latency_records` (in `stats/mod.rs`) ingests from diagnostics
records. Those records carry `success: bool` (the `LatencyMetricRecord.success`
field). Skip records where `!record.success` so the one-time backfill matches
the live-gating behavior. The ingest function stays shared; only the caller
filters.

### A3. Tests + status index

- Add/update a state-layer characterization test asserting a failed insert does
  not bump `all_time_words`.
- Add a stats unit test that a failed `LatencyMetricRecord` is ignored during
  backfill.
- Update the accuracy note at the bottom of `plans/007-stats-dashboard.md`.

---

## Part B — GitHub-style streak heatmap

### B1. Backend: expose per-day rollups

Today `get_stats_summary` returns an aggregated `StatsSummary` with no per-day
array. Add a method `day_buckets()` on `StatsManager` returning the last N
(e.g. 26 weeks = 182 days, GitHub-scale) as a compact, ordered list:

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayBucket {
    pub date: String,       // "YYYY-MM-DD"
    pub words: u64,
    pub dictations: u32,
    pub app_classes: Vec<(String, u32)>, // top-3 for hover tooltip
}
```

Extend `StatsSummary` (or add a sibling `StatsDetail`) with:
- `days: Vec<DayBucket>` (chronological, last 26 weeks including today)
- `longest_streak_days: u32`
- `current_streak_days: u32` (consecutive active days ending today or ending
  yesterday — GitHub counts a streak as "still alive" if today isn't over yet)
- `total_active_days` (already derivable from `active_days`)

**Streak math** (pure, on the ordered bucket list):
- `current_streak`: walk back from today; if today has dictations count it and
  continue; else only continue from yesterday (don't break a live streak on a
  partial day). Stop at first gap.
- `longest_streak`: scan the whole list for the longest run of non-empty days.
- Active = a day with `dictations > 0` (same definition as existing
  `active_days`).

Computed in Rust, unit-tested with the existing in-memory ingest harness.

### B2. New frontend component `StreakHeatmap.tsx`

New file under `src/components/`. Renders a GitHub-style contribution grid:

- **Layout**: 7 rows (Mon→Sun) × N week columns, right-aligned so the current
  week ends at the right edge. Each cell = one day.
- **Color ramp**: level 0 (inactive) → 4 (peak activity) mapped to the app's
  design language. The current Stats accent is the blue gradient
  (`#0A2A8C → #1B8EFF`). Proposal: keep it on-brand —
  - 0: `#E8E8EE` (quiet track, same as the WPM gauge's empty arc)
  - 1–4: anopacity ramp of the brand blue (`#1B8EFF` at 25/45/70/100), so it
    reads as part of the existing Stats design, not a foreign GitHub green.
  (If the maintainer prefers a green ramp later, it's a one-line palette change.)
- **Intensity binning**: word-count buckets. Simple and robust: 0 / 1–49 /
  50–199 / 200–499 / 500+ words per day. (Or quantile-based — I'll recommend
  fixed buckets for predictability and testability, matching how the store is
  already bucketed.)
- **Consecutive-day glow**: when the streak is "alive" (current_streak > 0),
  the most recent N cells get a soft brand ring/halo. This is the "glow on
  current streak" from plan-007 Tier 2.
- **Hover tooltip**: `Words: N · Dictations: M · Top app: X` on the hovered day
  (uses `app_classes` from the bucket). Native CSS title for zero-dependency, or
  a tiny positioned tooltip — decide in implementation, keep it dependency-free.
- **Weekday labels**: Mon/Wed/Fri on the left, GitHub-style.
- **Month labels**: optional light row-footer; include if it doesn't fight the
  layout.

### B3. Wire into the Stats tab

In `StatsSection.tsx`, add the heatmap as a full-width card under the hero row,
plus two habit chips above it: **`{current_streak} day streak`** (with the glow
when alive) and **`Longest: {longest_streak} days`**. Refresh via the existing
`listenVoicewaveHistoryUpdated` listener (no new plumbing needed — it already
re-fetches on every history update).

### B4. Data-accuracy contract for the heatmap

- Source is the same `stats.json` per-day rollups (already always-on, plain
  JSON, no text).
- Only `success == true` insertions count (per Part A).
- No backfill dependency: days before the store existed simply show as level 0 —
  the heatmap fills left-to-right as habits form. (Optional: the existing
  one-time diagnostics backfill already populates historical days where the
  user opted in.)

---

## Part C — Insight panels (plan-007 Tier 2/3)

### C1. "Where you dictate" — top app classes

Data already collected (`DayStats.app_classes`). Add `top_app_classes` to the
summary (aggregate the app_classes counts across all days, sort desc, take
top 4). Render as gradient bar rows (brand-blue width proportional to count),
mirroring the plan-007 spec. No new collection needed.

### C2. "Words cleaned up"

`raw_words` vs `final_words` are already recorded per day. Add
`words_cleaned = Σ(raw_words − final_words)` (clamped ≥ 0) to the summary.
Render as a friendly chip/panel: **"412 filler words never made it to the
page"**. Honest framing — this is the deterministic formatter + AI polish
difference, not a hard "error count."

### C3. "Clarity score" (the one piece needing new collection)

The friendly 0–100 "how clearly VoiceWave hears you". Needs signal that today
lives only in the **opt-in diagnostics** store: `fw_avg_logprob`,
`fw_no_speech_prob`, `fw_compression_ratio`. Options:

- **Option 1 (recommended, consistent with stats philosophy)**: carry a tiny
  rolling aggregate into `stats.json` at the same post-dictation seam — e.g.
  per-day sums of `avg_logprob` and `no_speech_prob` and a count, holidays-free.
  But note these fields are `Option` and come specifically from the
  faster-whisper telemetry, so the score is a best-effort "recent confidence"
  number, not universal.
- **Option 2 (cheaper)**: compute clarity only from the opt-in diagnostics
  store when present, and show a subtle "clarity data requires diagnostics"
  nudge otherwise. Less consistent, but zero new always-on storage.

I recommend **Option 1** so every user sees the score, but flag it clearly as
"best-effort, model-confidence-based" in the UI footnote (like the 40 WPM
baseline — honest math).

**Score mapping** (0–100, tuned in a spike, then unit-tested):
```
clarity = clamp(
  100 * (1 - |avg_logprob| / 1.0)        // closer to 0 dB → clearer
    * (1 - min(no_speech_prob, 0.5)/0.5) // speech confidence
    , 0, 100)
```
Aggregate as a rolling 7-day average so it doubles as the plan-007 "7-day
sparkline / early warning for the recurring Windows mic-volume drops."

---

## Part D — Scope control & non-goals

**In scope**: Part A (gate), Part B (heatmap + streak), Part C1/C2 (render
already-collected data), Part C3 (clarity aggregate + score).

**Explicitly out of scope** (do not slip in):
- No leaderboards (needs cloud/accounts — already rejected in 007).
- No content-category breakdowns (requires reading text — rejected in 007).
- No new store schema migration work beyond additive fields.
- No cloud sync of stats (stats stay device-local by design).

---

## Part E — Verification & acceptance

For each part, run the existing Rust test suite + frontend vitest:

```powershell
npm run test            # frontend vitest
npm run tauri:check     # rust compile + unit tests (via cargo_check harness)
```

1. **A**: test that a failed insert does not bump `all_time_words`; a failed
   `LatencyMetricRecord` is skipped in backfill.
2. **B**: unit tests for `current_streak` (live/day-partial/broken), `longest_streak`,
   day-bucket ordering; a rendered-heatmap snapshot/component test in vitest.
3. **C**: summary aggregation tests for top_app_classes, words_cleaned, clarity
   rolling average and its 0–100 clamp.
4. Manual smoke: dictate across two days, open Stats, confirm heatmap cells,
   streak count, and all three insight panels render with live data; confirm a
   failed-insertion dictation (if reproducible) does not move numbers.
5. Update `plans/README.md` row for 013 and flip Tier 2/3 in `007` from TODO to
   DONE.

---

## Maintainer decisions (2026-08, confirmed)

1. **Heatmap color**: brand-blue ramp, and the whole addition must match the
   existing design language exactly — every icon, shape, shade, corner radius,
   shadow, and typography token reused from the current Stats/Onboard panels.
   No foreign colors or new ad-hoc styles. The heatmap and all new panels reuse
   the existing `vw-*` utilities, the Onyx palette tokens (`#E8E8EE` quiet
   track, `#1B8EFF` accent, `#0A2A8C` deep brand, `rounded-3xl`, the Manrope /
   Fraunces pair, `shadow-[0_1px_2px_rgba(9,9,11,0.04)]`), and lucide-react
   icons from the same set already used in `StatsSection.tsx` / `Layout.tsx`.
2. **Streak semantics**: GitHub-style — a streak stays alive through a partial
   current day even if today is empty (count today if active, else continue
   from yesterday).
3. **Clarity score**: Option 1 — add a tiny always-on rolling aggregate so every
   user sees the score, honestly labeled as best-effort model-confidence data.
4. **Heatmap range**: adjustable — a segmented range selector with **1 month /
   3 months / 1 year**, defaulting to 1 month. Backend serves the requested
   window; frontend keeps a small range toggle in the panel header style.

### Range selector (decision 4) — implementation notes

- Frontend: a 3-way segmented control reusing the existing `vw-chip` /
  `vw-chip-active` or nav-button style, in the heatmap panel header. It holds
  `'1m' | '3m' | '1y'` state and refetches the summary for that window.
- Backend: `day_buckets()` takes a `window_days: u32` (30 / 91 / 365) and
  returns the last N bucket rows. Streak math stays full-history (longest
  streak always reflects all data; only the rendered grid width changes).
  `current_streak` is window-independent by design.
- Default view: **1 month** (31 days — reads best on a heatmap and fills
  faster than 1 year of sparse early data).
