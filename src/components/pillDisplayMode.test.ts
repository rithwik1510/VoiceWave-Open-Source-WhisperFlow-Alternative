import { describe, expect, it } from "vitest";
import type { DictionaryQueueItem, PillNoticePayload } from "../types/voicewave";
import {
  derivePillDisplayMode,
  shouldAutoDismissQuickControls,
  shouldCloseQuickControlsOnKey
} from "./pillDisplayMode";

const review: DictionaryQueueItem = {
  entryId: "review-1",
  term: "VoiceWave",
  sourcePreview: "voice wave",
  createdAtUtcMs: 1
};

const notice = (recovery: boolean): PillNoticePayload => ({
  id: 1,
  severity: recovery ? "error" : "info",
  title: recovery ? "Insertion failed" : "Update ready",
  detail: null,
  durationMs: 5000,
  transcript: recovery ? "protected words" : null,
  action: null,
  recovery
});

function derive(overrides: Partial<Parameters<typeof derivePillDisplayMode>[0]> = {}) {
  return derivePillDisplayMode({
    visualState: "idle",
    quickControlsEnabled: false,
    quickControlsOpen: false,
    quickControlsInvocation: "mouse",
    reviewItem: null,
    notice: null,
    ...overrides
  });
}

describe("derivePillDisplayMode", () => {
  it("uses the documented recovery and lifecycle priority", () => {
    expect(
      derive({
        visualState: "listening",
        quickControlsEnabled: true,
        quickControlsOpen: true,
        reviewItem: review,
        notice: notice(true)
      }).kind
    ).toBe("notice");

    expect(
      derive({
        visualState: "listening",
        quickControlsEnabled: true,
        quickControlsOpen: true,
        reviewItem: review,
        notice: notice(false)
      }).kind
    ).toBe("status");
  });

  it("lets explicit Quick Controls temporarily preempt review and ordinary notices", () => {
    expect(
      derive({
        quickControlsEnabled: true,
        quickControlsOpen: true,
        reviewItem: review,
        notice: notice(false)
      })
    ).toEqual({ kind: "quickControls", invocation: "mouse" });
  });

  it("restores review before ordinary notices after Quick Controls closes", () => {
    expect(derive({ reviewItem: review, notice: notice(false) })).toEqual({
      kind: "review",
      item: review
    });
    expect(derive({ notice: notice(false) }).kind).toBe("notice");
    expect(derive()).toEqual({ kind: "status", visual: "idle" });
  });

  it("reserves timeout for mouse entry and Escape for focused entry", () => {
    expect(shouldAutoDismissQuickControls("mouse")).toBe(true);
    expect(shouldAutoDismissQuickControls("focused")).toBe(false);
    expect(shouldCloseQuickControlsOnKey("focused", "Escape")).toBe(true);
    expect(shouldCloseQuickControlsOnKey("mouse", "Escape")).toBe(false);
  });
});
