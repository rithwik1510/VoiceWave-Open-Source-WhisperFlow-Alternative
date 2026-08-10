import { act, render, screen, waitFor } from "@testing-library/react";
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
  activeDays: 49,
  // plan 013 additions.
  days: [
    {
      date: "2026-07-17",
      words: 320,
      dictations: 5,
      appClasses: [{ name: "Slack", count: 3 }]
    }
  ],
  longestStreakDays: 12,
  currentStreakDays: 3,
  topAppClasses: [
    { name: "Slack", count: 3 },
    { name: "VS Code", count: 2 }
  ],
  wordsCleanedUp: 412,
  clarityScore: 87,
  rangeDays: 30
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

  it("renders the streak heatmap and insight panels", async () => {
    const { StatsSection } = await import("./StatsSection");
    render(<StatsSection />);

    // Heatmap header + range selector.
    expect(await screen.findByText("Activity")).toBeInTheDocument();
    expect(screen.getByText("1 month")).toBeInTheDocument();
    expect(screen.getByText("3 months")).toBeInTheDocument();
    expect(screen.getByText("1 year")).toBeInTheDocument();
    // Streak chips (current alive + longest).
    expect(screen.getByText("3 day streak")).toBeInTheDocument();
    expect(screen.getByText("Longest: 12 days")).toBeInTheDocument();

    // Where you dictate (top app classes).
    expect(screen.getByText("Where you dictate")).toBeInTheDocument();
    expect(screen.getByText("Slack")).toBeInTheDocument();
    expect(screen.getByText("VS Code")).toBeInTheDocument();

    // Words cleaned up chip.
    expect(screen.getByText("412 filler words never made it to the page")).toBeInTheDocument();

    // Voice clarity (0-100).
    expect(screen.getByText("Voice clarity")).toBeInTheDocument();
    expect(screen.getByText(/87/)).toBeInTheDocument();
  });

  it("refetches with the selected range when the heatmap range changes", async () => {
    const { StatsSection } = await import("./StatsSection");
    render(<StatsSection />);

    await screen.findByText("Activity");
    expect(getStatsSummary).toHaveBeenLastCalledWith(30);

    const yearButton = screen.getByRole("button", { name: "1 year" });
    await act(() => {
      yearButton.click();
    });

    await waitFor(() => {
      expect(getStatsSummary).toHaveBeenLastCalledWith(365);
    });
  });
});
