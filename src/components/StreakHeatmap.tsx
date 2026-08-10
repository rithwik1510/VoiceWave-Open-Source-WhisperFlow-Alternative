import { useMemo, useState } from "react";
import { createPortal } from "react-dom";
import { Flame } from "lucide-react";
import type { DayBucket } from "../types/voicewave";

/* ------------------------------------------------------------------ *
 * GitHub-style contribution heatmap for the Stats tab (plan 013).
 * Design language matches the app: brand-blue ramp, Onyx palette tokens,
 * Manrope labels, vw-chip pills, rounded surfaces, subtle shadows.
 * ------------------------------------------------------------------ */

/** Word-count buckets → heatmap intensity level 0..4. */
function dayLevel(words: number): number {
  if (words <= 0) return 0;
  if (words < 50) return 1;
  if (words < 200) return 2;
  if (words < 500) return 3;
  return 4;
}

/** Brand-blue ramp (not GitHub green). Level 0 is the quiet track reuse.
 * The values live in CSS so the ramp gets a dark floor for free — these are
 * only ever used as `background-color`, where `var()` resolves normally. */
const LEVEL_COLORS: string[] = [
  "var(--vw-heat-empty)", // 0 · inactive (quiet track, same as the WPM gauge arc)
  "var(--vw-heat-1)", // 1 · 1–49 words
  "var(--vw-heat-2)", // 2 · 50–199
  "var(--vw-heat-3)", // 3 · 200–499
  "var(--vw-heat-4)", // 4 · 500+ · full brand accent
];

const WEEKDAY_LABELS = ["Mon", "Wed", "Fri"];

/** Fixed square cell size (px) for every range. Short ranges stay calm and
 * compact; they show fewer columns rather than stretching across the card. */
const CELL_PX = 12;
const CELL_GAP = 3;

type RangeKey = "1m" | "3m" | "1y";

const RANGES: { key: RangeKey; label: string; days: number }[] = [
  { key: "1m", label: "1 month", days: 30 },
  { key: "3m", label: "3 months", days: 91 },
  { key: "1y", label: "1 year", days: 365 },
];

function rangeKeyFor(rangeDays: number): RangeKey {
  if (rangeDays >= 300) return "1y";
  if (rangeDays >= 60) return "3m";
  return "1m";
}

interface StreakHeatmapProps {
  days: DayBucket[];
  currentStreak: number;
  longestStreak: number;
  rangeDays: number;
  onRangeChange: (rangeDays: number) => void;
}

/**
 * Renders the days as an 7-row (Mon..Sun) × week-columns grid, right-aligned so
 * the current (partial) week sits at the far right edge — GitHub style.
 */
export function StreakHeatmap({
  days,
  currentStreak,
  longestStreak,
  rangeDays,
  onRangeChange,
}: StreakHeatmapProps) {
  const [hovered, setHovered] = useState<DayBucket | null>(null);
  const [hoverPos, setHoverPos] = useState<{
    x: number;
    y: number;
    side: "above" | "below";
  } | null>(null);

  const activeRange = rangeKeyFor(rangeDays);

  /* Build a date → bucket map, then lay the grid out in week columns that
   * start on Monday and end on the current calendar week. */
  const { weeks, totalWords } = useMemo(() => {
    const byDate = new Map<string, DayBucket>();
    let total = 0;
    for (const d of days) {
      byDate.set(d.date, d);
      total += d.words;
    }

    const now = new Date();
    // Find the Monday of the current week.
    const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
    const dow = (today.getDay() + 6) % 7; // Mon=0 ... Sun=6
    const thisMonday = new Date(today);
    thisMonday.setDate(today.getDate() - dow);

    // Walk back `rangeDays` from today to find the first Monday to render.
    const windowStart = new Date(today);
    windowStart.setDate(today.getDate() - (rangeDays - 1));
    const firstMonday = new Date(windowStart);
    firstMonday.setDate(windowStart.getDate() - (windowStart.getDay() + 6) % 7);

    const weeksOut: (DayBucket | null)[][] = [];
    const cursor = new Date(firstMonday);
    while (cursor <= thisMonday) {
      const col: (DayBucket | null)[] = [];
      for (let i = 0; i < 7; i++) {
        const key = `${cursor.getFullYear()}-${String(cursor.getMonth() + 1).padStart(2, "0")}-${String(cursor.getDate()).padStart(2, "0")}`;
        col.push(byDate.get(key) ?? null);
        cursor.setDate(cursor.getDate() + 1);
      }
      weeksOut.push(col);
    }
    return { weeks: weeksOut, totalWords: total };
  }, [days, rangeDays]);

  const streakAlive = currentStreak > 0;

  /** Position a viewport-pinned tooltip (rendered via a portal to <body> so no
   *  scroll container or transformed ancestor can clip/drift it). It hugs the
   *  hovered square: centers horizontally over the cell, shows just above it,
   *  and flips below when the cell sits too near the top of the viewport. */
  const handleHover = (
    bucket: DayBucket | null,
    e: React.MouseEvent<HTMLDivElement>
  ) => {
    if (!bucket || bucket.dictations === 0) {
      setHovered(null);
      setHoverPos(null);
      return;
    }
    const rect = e.currentTarget.getBoundingClientRect();
    const centerX = rect.left + rect.width / 2;
    const estH = 78; // approximate tooltip height for flip / clamping
    const roomAbove = rect.top - 8;
    const roomBelow = window.innerHeight - rect.bottom - 8;
    const side = roomAbove >= estH || roomAbove >= roomBelow ? "above" : "below";
    const vw = window.innerWidth;
    // Clamp horizontally so the ~140px tooltip stays fully on-screen.
    const x = Math.min(Math.max(centerX, 150), vw - 150);
    setHovered(bucket);
    setHoverPos({
      x,
      // Anchor the tooltip edge 8px off the cell, on whichever side was chosen.
      y: side === "above" ? rect.top - 8 : rect.bottom + 8,
      side,
    });
  };

  return (
    <div className="rounded-3xl border border-edge bg-surface px-6 py-5 shadow-[var(--vw-shadow-card)]">
      {/* Header: title + range selector */}
      <div className="flex flex-wrap items-center justify-between gap-3">
        <p className="flex items-center gap-1.5 text-xs font-semibold uppercase tracking-[0.16em] text-faint">
          <Flame size={13} className="text-accent" />
          Activity
        </p>
        <div
          className="flex shrink-0 items-center gap-0.5 rounded-full border border-edge bg-inset p-0.5"
          aria-label="Activity date range"
        >
          {RANGES.map((r) => {
            const isActive = activeRange === r.key;
            return (
              <button
                key={r.key}
                type="button"
                onClick={() => onRangeChange(r.days)}
                className={`rounded-full px-2.5 py-1 text-[11px] font-semibold transition-colors duration-150 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent ${
                  isActive
                    ? "bg-accent-deep text-on-accent ring-1 ring-inset ring-accent-rule shadow-[var(--vw-shadow-card)]"
                    : "text-faint hover:bg-surface hover:text-ink-strong"
                }`}
              >
                {r.label}
              </button>
            );
          })}
        </div>
      </div>

      {/* Streak chips */}
      <div className="mt-3 flex flex-wrap items-center gap-2">
        {streakAlive ? (
          <span className="vw-chip vw-chip-accent">
            <Flame size={11} />
            {currentStreak} day streak
          </span>
        ) : (
          <span className="vw-chip">No active streak</span>
        )}
        {longestStreak > 0 && (
          <span className="vw-chip">Longest: {longestStreak} days</span>
        )}
        <span className="ml-auto text-[11px] text-hint">
          {totalWords.toLocaleString("en-US")} words in view
        </span>
      </div>

      {/* The grid. The outer wrapper is the only scroll container (for narrow
          screens + the one-year range) and deliberately hides its scrollbar.
          Fixed tracks and non-scaling cells keep the layout stable on hover. */}
      <div className="mt-4 min-w-0 overflow-x-auto overscroll-x-contain pb-1 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
        <div className="flex w-max items-start gap-3 pr-1">
          {/* Weekday labels */}
          <div className="flex flex-col justify-between py-[1px]">
            {WEEKDAY_LABELS.map((label) => (
              <span
                key={label}
                className="flex h-3 items-center text-[9px] font-medium uppercase tracking-wide text-hint"
              >
                {label}
              </span>
            ))}
          </div>

          {/* Cells (7 rows × N week columns), always left-aligned and fixed-size. */}
          <div
            className="grid"
            style={{
              gridTemplateColumns: `repeat(${weeks.length}, ${CELL_PX}px)`,
              gap: `${CELL_GAP}px`,
            }}
          >
            {Array.from({ length: 7 }).map((_, row) => (
              <div key={row} className="contents">
                {weeks.map((week, col) => {
                  const bucket = week[row];
                  if (!bucket) {
                    return (
                      <div
                        key={col}
                        className="rounded-[3px]"
                        style={{
                          width: CELL_PX,
                          height: CELL_PX,
                          backgroundColor: LEVEL_COLORS[0],
                        }}
                      />
                    );
                  }
                  const level = dayLevel(bucket.words);
                  const isStreakCell =
                    streakAlive &&
                    currentStreak > 0 &&
                    bucket.dictations > 0 &&
                    // this week is the streak's tail zone (active recent days glow)
                    col >= Math.max(0, weeks.length - currentStreak) &&
                    bucket.words > 0;
                  return (
                    <div
                      key={col}
                      className="rounded-[3px] transition-[filter,box-shadow] duration-150 hover:brightness-90"
                      style={{
                        width: CELL_PX,
                        height: CELL_PX,
                        backgroundColor: LEVEL_COLORS[level],
                        boxShadow: isStreakCell
                          ? "0 0 0 1.5px var(--vw-accent-blue-600)"
                          : undefined,
                      }}
                      onMouseEnter={(e) => handleHover(bucket, e)}
                      onMouseLeave={() => {
                        setHovered(null);
                        setHoverPos(null);
                      }}
                    />
                  );
                })}
              </div>
            ))}
          </div>
        </div>
      </div>

      {/* Footnote */}
      <div className="ml-8 mt-3 flex items-center justify-start gap-1.5">
        <span className="text-[10px] text-hint">Less</span>
        {LEVEL_COLORS.map((c, i) => (
          <span
            key={i}
            className="h-2.5 w-2.5 rounded-[2px]"
            style={{ backgroundColor: c }}
          />
        ))}
        <span className="text-[10px] text-hint">More</span>
      </div>

      {/* Hover tooltip — rendered into <body> via a portal so nothing can clip
          or misplace it. Anchored to the hovered square, flipping below when
          the cell is near the top of the viewport. */}
      {hovered &&
        hoverPos &&
        createPortal(
          <div
            className="pointer-events-none fixed z-[999] bg-ink px-3 py-2 text-xs text-ink-contrast shadow-lg"
            style={{
              left: hoverPos.x,
              top: hoverPos.y,
              transform:
                hoverPos.side === "above"
                  ? "translate(-50%, -100%)"
                  : "translate(-50%, 0)",
              borderRadius: 8,
            }}
            role="tooltip"
          >
            <p className="font-semibold">
              {hovered.dictations}{" "}
              {hovered.dictations === 1 ? "dictation" : "dictations"}
              {hovered.words > 0 &&
                ` · ${hovered.words.toLocaleString("en-US")} words`}
            </p>
            {/* The tooltip fill inverts with the theme, so its secondary tiers
                step down from the contrast color rather than using the
                page-relative text tokens. */}
            {hovered.appClasses.length > 0 && (
              <p className="mt-0.5 text-ink-contrast opacity-80">
                Top: {hovered.appClasses[0].name}
              </p>
            )}
            <p className="mt-0.5 text-ink-contrast opacity-60">{hovered.date}</p>
          </div>,
          document.body
        )}
    </div>
  );
}
