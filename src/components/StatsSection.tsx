import { useEffect, useRef, useState } from "react";
import {
  AudioLines,
  Clock,
  Gauge,
  MapPin,
  Sparkles,
  TrendingDown,
  TrendingUp,
  Type,
} from "lucide-react";

import { canUseTauri, getStatsSummary, listenVoicewaveHistoryUpdated } from "../lib/tauri";
import type { StatsSummary } from "../types/voicewave";
import { StreakHeatmap } from "./StreakHeatmap";

/** Default heatmap / stats window. Backend normalizes to 30 | 91 | 365. */
const DEFAULT_RANGE_DAYS = 30;

/** Gauge scale ceiling. 200 WPM is very fast sustained speech; the arc caps
 * there so ordinary values (100-160) read as meaningful progress. */
const GAUGE_MAX_WPM = 200;

function prefersReducedMotion(): boolean {
  return (
    typeof window !== "undefined" &&
    typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches
  );
}

/** Animate 0 → 1 with an ease-out curve; returns instantly-1 under reduced
 * motion. Drives every count-up on the page from a single RAF. */
function useRiseProgress(durationMs = 1000): number {
  const [progress, setProgress] = useState(() => (prefersReducedMotion() ? 1 : 0));

  useEffect(() => {
    if (progress >= 1) {
      return;
    }
    let raf = 0;
    const started = performance.now();
    const frame = (now: number) => {
      const linear = Math.min(1, (now - started) / durationMs);
      const eased = 1 - Math.pow(1 - linear, 3);
      setProgress(eased);
      if (linear < 1) {
        raf = requestAnimationFrame(frame);
      }
    };
    raf = requestAnimationFrame(frame);
    return () => cancelAnimationFrame(raf);
    // Intentionally run once on mount; `progress` only guards re-entry.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return progress;
}

function formatDuration(ms: number): string {
  const totalMinutes = Math.round(ms / 60_000);
  if (totalMinutes < 1) {
    return ms > 0 ? "<1m" : "0m";
  }
  const hours = Math.floor(totalMinutes / 60);
  const minutes = totalMinutes % 60;
  if (hours === 0) {
    return `${minutes}m`;
  }
  return minutes === 0 ? `${hours}h` : `${hours}h ${minutes}m`;
}

function formatWords(count: number): string {
  return Math.round(count).toLocaleString("en-US");
}

/** Semicircular WPM gauge: quiet track, brand-gradient arc that sweeps in. */
function WpmGauge({ wpm, progress }: { wpm: number; progress: number }) {
  const radius = 84;
  const stroke = 13;
  const circumference = Math.PI * radius;
  const fraction = Math.max(0, Math.min(1, wpm / GAUGE_MAX_WPM)) * progress;

  return (
    <svg
      viewBox="0 0 220 122"
      className="w-full max-w-[240px]"
      role="img"
      aria-label={`Average speaking speed ${Math.round(wpm)} words per minute`}
    >
      <defs>
        <linearGradient id="vw-stats-arc" x1="0%" y1="100%" x2="100%" y2="0%">
          <stop offset="0%" stopColor="#0A2A8C" />
          <stop offset="55%" stopColor="#0F5FD7" />
          <stop offset="100%" stopColor="#1B8EFF" />
        </linearGradient>
      </defs>
      <path
        d={`M ${110 - radius} 110 A ${radius} ${radius} 0 0 1 ${110 + radius} 110`}
        fill="none"
        stroke="#E8E8EE"
        strokeWidth={stroke}
        strokeLinecap="round"
      />
      <path
        d={`M ${110 - radius} 110 A ${radius} ${radius} 0 0 1 ${110 + radius} 110`}
        fill="none"
        stroke="url(#vw-stats-arc)"
        strokeWidth={stroke}
        strokeLinecap="round"
        strokeDasharray={circumference}
        strokeDashoffset={circumference * (1 - fraction)}
      />
      <text
        x="110"
        y="92"
        textAnchor="middle"
        className="fill-[#09090B]"
        style={{ font: '600 40px "Manrope", sans-serif' }}
      >
        {Math.round(wpm * progress)}
      </text>
      <text
        x="110"
        y="112"
        textAnchor="middle"
        className="fill-[#A1A1AA]"
        style={{ font: "600 11px 'Manrope', sans-serif", letterSpacing: "0.14em" }}
      >
        WPM
      </text>
    </svg>
  );
}

export function StatsSection() {
  const [summary, setSummary] = useState<StatsSummary | null>(null);
  const [unavailable, setUnavailable] = useState(!canUseTauri());
  const [range, setRange] = useState(DEFAULT_RANGE_DAYS);
  const progress = useRiseProgress();
  const mountedRef = useRef(true);

  useEffect(() => {
    mountedRef.current = true;
    if (!canUseTauri()) {
      return;
    }
    const refresh = async () => {
      try {
        const next = await getStatsSummary(range);
        if (mountedRef.current) {
          setSummary(next);
        }
      } catch {
        if (mountedRef.current) {
          setUnavailable(true);
        }
      }
    };
    void refresh();
    let unlisten: (() => void) | null = null;
    void listenVoicewaveHistoryUpdated(() => {
      void refresh();
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      mountedRef.current = false;
      if (unlisten) {
        unlisten();
      }
    };
  }, [range]);

  const handleRangeChange = (next: number) => {
    if (next !== range) {
      setRange(next);
    }
  };

  const empty = !summary || summary.allTimeDictations === 0;

  if (unavailable || empty) {
    return (
      <section className="vw-panel vw-panel-soft">
        <p className="vw-kicker">On-Device</p>
        <h3 className="vw-section-heading mt-1 text-2xl font-semibold text-[#09090B]">Stats</h3>
        <div className="mt-6 rounded-2xl border border-dashed border-[#E4E4E7] px-6 py-12 text-center">
          <p className="text-sm font-medium text-[#09090B]">No numbers yet</p>
          <p className="mt-1 text-sm text-[#71717A]">
            {unavailable
              ? "Stats are computed inside the desktop app."
              : "Dictate once and your numbers appear here."}
          </p>
        </div>
      </section>
    );
  }

  const monthDelta =
    summary.prevMonthWords > 0
      ? Math.round(((summary.monthWords - summary.prevMonthWords) / summary.prevMonthWords) * 100)
      : null;
  const speedMultiple =
    summary.typingBaselineWpm > 0 ? summary.averageWpm / summary.typingBaselineWpm : 0;

  return (
    <section className="vw-panel vw-panel-soft">
      <p className="vw-kicker">On-Device</p>
      <h3 className="vw-section-heading mt-1 text-2xl font-semibold text-[#09090B]">Stats</h3>
      <p className="mt-1 text-sm text-[#71717A]">
        Computed from anonymous daily totals on this machine — nothing leaves it.
      </p>

      <div className="vw-list-stagger mt-5 space-y-4">
        {/* Hero: time saved. */}
        <div className="vw-ring-shell vw-ring-shell-lg">
          <div className="vw-ring-inner px-7 py-6">
            <div className="flex flex-wrap items-end justify-between gap-6">
              <div>
                <p className="flex items-center gap-1.5 text-xs font-semibold uppercase tracking-[0.16em] text-[#71717A]">
                  <Clock size={13} className="text-[#1B8EFF]" />
                  Time saved vs typing
                </p>
                <p className='mt-2 font-["Fraunces"] text-[54px] leading-none tracking-tight text-[#09090B]'>
                  {formatDuration(summary.timeSavedMsAllTime * progress)}
                </p>
                <p className="mt-2 text-sm text-[#71717A]">
                  {formatDuration(summary.timeSavedMsMonth)} this month ·{" "}
                  {formatDuration(summary.speakingMs)} spent speaking in total
                </p>
              </div>
              <div className="text-right">
                <p className="text-xs font-semibold uppercase tracking-[0.16em] text-[#71717A]">
                  Dictations
                </p>
                <p className="mt-1 text-2xl font-semibold tabular-nums text-[#09090B]">
                  {formatWords(summary.allTimeDictations * progress)}
                </p>
                <p className="mt-1 text-xs text-[#A1A1AA]">
                  across {summary.activeDays} active {summary.activeDays === 1 ? "day" : "days"}
                </p>
              </div>
            </div>
          </div>
        </div>

        <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
          {/* Speaking speed gauge. */}
          <div className="rounded-3xl border border-[#E4E4E7] bg-white px-6 py-5 shadow-[0_1px_2px_rgba(9,9,11,0.04)]">
            <p className="flex items-center gap-1.5 text-xs font-semibold uppercase tracking-[0.16em] text-[#71717A]">
              <Gauge size={13} className="text-[#1B8EFF]" />
              Speaking speed
            </p>
            <div className="mt-3 flex justify-center">
              <WpmGauge wpm={summary.averageWpm} progress={progress} />
            </div>
            <div className="mt-3 flex flex-wrap items-center justify-center gap-2">
              {speedMultiple >= 1.05 && (
                <span className="vw-chip vw-chip-accent">
                  {speedMultiple.toFixed(1)}× faster than typing
                </span>
              )}
              {summary.bestDictationWpm > 0 && (
                <span className="vw-chip">Best: {Math.round(summary.bestDictationWpm)} WPM</span>
              )}
            </div>
          </div>

          {/* Words dictated. */}
          <div className="rounded-3xl border border-[#E4E4E7] bg-white px-6 py-5 shadow-[0_1px_2px_rgba(9,9,11,0.04)]">
            <div className="flex items-center justify-between gap-2">
              <p className="flex items-center gap-1.5 text-xs font-semibold uppercase tracking-[0.16em] text-[#71717A]">
                <Type size={13} className="text-[#1B8EFF]" />
                Words dictated
              </p>
              {monthDelta !== null && (
                <span
                  className={`flex items-center gap-1 rounded-full px-2 py-0.5 text-[11px] font-semibold ${
                    monthDelta >= 0 ? "bg-[#E8F4FF] text-[#0A2A8C]" : "bg-[#F4F4F5] text-[#71717A]"
                  }`}
                >
                  {monthDelta >= 0 ? <TrendingUp size={11} /> : <TrendingDown size={11} />}
                  {Math.abs(monthDelta)}% vs last month
                </span>
              )}
            </div>
            <p className='mt-4 font-["Fraunces"] text-[44px] leading-none tracking-tight text-[#09090B]'>
              {formatWords(summary.allTimeWords * progress)}
            </p>
            <p className="mt-1 text-xs text-[#A1A1AA]">all time</p>
            <div className="mt-4 grid grid-cols-2 gap-3">
              <div className="rounded-2xl bg-[#FAFAFA] px-4 py-3">
                <p className="text-[11px] font-semibold uppercase tracking-[0.14em] text-[#A1A1AA]">
                  Today
                </p>
                <p className="mt-1 text-lg font-semibold tabular-nums text-[#09090B]">
                  {formatWords(summary.todayWords)}
                </p>
              </div>
              <div className="rounded-2xl bg-[#FAFAFA] px-4 py-3">
                <p className="text-[11px] font-semibold uppercase tracking-[0.14em] text-[#A1A1AA]">
                  This week
                </p>
                <p className="mt-1 text-lg font-semibold tabular-nums text-[#09090B]">
                  {formatWords(summary.weekWords)}
                </p>
              </div>
            </div>
          </div>
        </div>
      </div>

      {/* Plan 013: heatmap + insight panels (streak, where you dictate,
          words cleaned up, clarity). All styled to match the app design
          language (brand-blue ramp, Onyx tokens, vw-* utilities). */}
      <div className="mt-5 space-y-4">
        <StreakHeatmap
        days={summary.days}
        currentStreak={summary.currentStreakDays}
        longestStreak={summary.longestStreakDays}
        rangeDays={summary.rangeDays}
        onRangeChange={handleRangeChange}
      />

      {/* Insight: where you dictate (top app classes). */}
      {summary.topAppClasses.length > 0 && (
        <div className="rounded-3xl border border-[#E4E4E7] bg-white px-6 py-5 shadow-[0_1px_2px_rgba(9,9,11,0.04)]">
          <p className="flex items-center gap-1.5 text-xs font-semibold uppercase tracking-[0.16em] text-[#71717A]">
            <MapPin size={13} className="text-[#1B8EFF]" />
            Where you dictate
          </p>
          <div className="mt-4 space-y-3">
            {summary.topAppClasses.map((app) => {
              const max = summary.topAppClasses[0].count || 1;
              const width = `${Math.max(6, Math.round((app.count / max) * 100))}%`;
              return (
                <div key={app.name} className="flex items-center gap-3">
                  <span className="w-20 shrink-0 truncate text-xs font-medium text-[#09090B] capitalize">
                    {app.name}
                  </span>
                  <div className="h-2 flex-1 overflow-hidden rounded-full bg-[#E8E8EE]">
                    <div
                      className="h-full rounded-full"
                      style={{
                        width,
                        background:
                          "linear-gradient(90deg,#0A2A8C,#1B8EFF)",
                      }}
                    />
                  </div>
                  <span className="w-10 shrink-0 text-right text-xs tabular-nums text-[#71717A]">
                    {app.count.toLocaleString("en-US")}
                  </span>
                </div>
              );
            })}
          </div>
        </div>
      )}

      {/* Insight: words cleaned up (raw − final). */}
      {summary.wordsCleanedUp > 0 && (
        <div className="vw-chip vw-chip-accent">
          <Sparkles size={11} />
          {formatWords(summary.wordsCleanedUp)} filler words never made it to the page
        </div>
      )}

      {/* Insight: voice clarity (0–100, best-effort confidence estimate). */}
      <div className="rounded-3xl border border-[#E4E4E7] bg-white px-6 py-5 shadow-[0_1px_2px_rgba(9,9,11,0.04)]">
        <div className="flex items-center justify-between gap-2">
          <p className="flex items-center gap-1.5 text-xs font-semibold uppercase tracking-[0.16em] text-[#71717A]">
            <AudioLines size={13} className="text-[#1B8EFF]" />
            Voice clarity
          </p>
          <span className="text-2xl font-semibold tabular-nums text-[#09090B]">
            {Math.round(summary.clarityScore)}
            <span className="text-sm text-[#A1A1AA]">/100</span>
          </span>
        </div>
        <div className="mt-3 h-2 w-full overflow-hidden rounded-full bg-[#E8E8EE]">
          <div
            className="h-full rounded-full transition-all duration-700"
            style={{
              width: `${Math.max(0, Math.min(100, summary.clarityScore))}%`,
              background: "linear-gradient(90deg,#0A2A8C,#1B8EFF)",
            }}
          />
        </div>
        <p className="mt-2 text-[11px] text-[#A1A1AA]">
          Model-confidence estimate of how clearly VoiceWave hears you.
        </p>
      </div>
      </div>

      <p className="mt-4 text-xs text-[#A1A1AA]">
        Time saved compares your measured speaking speed with {summary.typingBaselineWpm} WPM
        average typing. Dictations under 2 seconds aren't counted.
      </p>
    </section>
  );
}
