import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { StatsSummary } from "../types/voicewave";

const SUMMARY: StatsSummary = {
  todayWords: 320,
  weekWords: 2140,
  monthWords: 4000,
  prevMonthWords: 3200,
  allTimeWords: 58316,
  allTimeDictations: 1959,
  speakingMs: 439 * 60_000,
  averageWpm: 132.6,
  bestDictationWpm: 185.2,
  // 58316 words at 40wpm typing = 1457.9 min; minus 439 min speaking.
  timeSavedMsAllTime: Math.round((58316 / 40 - 439) * 60_000),
  timeSavedMsMonth: 42 * 60_000,
  typingBaselineWpm: 40,
  activeDays: 49
};

const getStatsSummary = vi.fn();

vi.mock("../lib/tauri", () => ({
  canUseTauri: () => true,
  getStatsSummary: (...args: unknown[]) => getStatsSummary(...args),
  listenVoicewaveHistoryUpdated: vi.fn().mockResolvedValue(() => undefined)
}));

// Reduced motion => count-ups render final values immediately, so the test
// asserts exact formatted numbers instead of animation frames.
beforeEach(() => {
  window.matchMedia = vi.fn().mockImplementation((query: string) => ({
    matches: query.includes("prefers-reduced-motion"),
    media: query,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    addListener: vi.fn(),
    removeListener: vi.fn(),
    onchange: null,
    dispatchEvent: vi.fn()
  }));
  getStatsSummary.mockResolvedValue(SUMMARY);
});

describe("StatsSection hero tier", () => {
  it("renders exact formatted hero numbers from the summary", async () => {
    const { StatsSection } = await import("./StatsSection");
    render(<StatsSection />);

    // Time saved: 1457.9 - 439 = 1018.9 min -> 1019 min -> 16h 59m.
    expect(await screen.findByText("16h 59m")).toBeInTheDocument();
    expect(screen.getByText(/42m this month/)).toBeInTheDocument();
    expect(screen.getByText("58,316")).toBeInTheDocument();
    expect(screen.getByText("1,959")).toBeInTheDocument();
    expect(screen.getByText("across 49 active days")).toBeInTheDocument();
    expect(screen.getByText("3.3× faster than typing")).toBeInTheDocument();
    expect(screen.getByText("Best: 185 WPM")).toBeInTheDocument();
    expect(screen.getByText("320")).toBeInTheDocument();
    expect(screen.getByText("2,140")).toBeInTheDocument();
    expect(screen.getByText("25% vs last month")).toBeInTheDocument();
    expect(
      screen.getByRole("img", { name: "Average speaking speed 133 words per minute" })
    ).toBeInTheDocument();
  });

  it("shows the empty state before any dictation", async () => {
    getStatsSummary.mockResolvedValue({ ...SUMMARY, allTimeDictations: 0, allTimeWords: 0 });
    const { StatsSection } = await import("./StatsSection");
    render(<StatsSection />);

    expect(await screen.findByText("No numbers yet")).toBeInTheDocument();
    expect(screen.getByText("Dictate once and your numbers appear here.")).toBeInTheDocument();
  });
});
