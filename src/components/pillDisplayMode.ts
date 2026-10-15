import type {
  DictionaryQueueItem,
  PillNoticePayload,
  PillQuickControlsInvocation,
  VoiceWaveHudState
} from "../types/voicewave";

export type PillDisplayMode =
  | { kind: "status"; visual: VoiceWaveHudState }
  | { kind: "quickControls"; invocation: PillQuickControlsInvocation }
  | { kind: "review"; item: DictionaryQueueItem }
  | { kind: "notice"; notice: PillNoticePayload; recovery: boolean };

interface DerivePillDisplayModeInput {
  visualState: VoiceWaveHudState;
  quickControlsEnabled: boolean;
  quickControlsOpen: boolean;
  quickControlsInvocation: PillQuickControlsInvocation;
  reviewItem: DictionaryQueueItem | null;
  notice: PillNoticePayload | null;
}

/** One deterministic owner for the pill surface. State is deliberately not
 * mutated here: lower-priority review/notice content remains queued in React
 * and becomes visible again after the higher-priority surface leaves. */
export function derivePillDisplayMode({
  visualState,
  quickControlsEnabled,
  quickControlsOpen,
  quickControlsInvocation,
  reviewItem,
  notice
}: DerivePillDisplayModeInput): PillDisplayMode {
  if (notice?.recovery) {
    return { kind: "notice", notice, recovery: true };
  }

  if (visualState !== "idle") {
    return { kind: "status", visual: visualState };
  }

  if (quickControlsEnabled && quickControlsOpen) {
    return { kind: "quickControls", invocation: quickControlsInvocation };
  }

  if (reviewItem) {
    return { kind: "review", item: reviewItem };
  }

  if (notice) {
    return { kind: "notice", notice, recovery: false };
  }

  return { kind: "status", visual: "idle" };
}

export function shouldAutoDismissQuickControls(
  invocation: PillQuickControlsInvocation
): boolean {
  return invocation === "mouse";
}

export function shouldCloseQuickControlsOnKey(
  invocation: PillQuickControlsInvocation,
  key: string
): boolean {
  return invocation === "focused" && key === "Escape";
}
