import type { VoiceWaveHudState } from "../types/voicewave";

export function stateLabel(state: VoiceWaveHudState): string {
  switch (state) {
    case "idle":
      return "Idle";
    case "listening":
      return "Listening";
    case "transcribing":
      return "Transcribing";
    case "inserted":
      return "Inserted";
    case "error":
      return "Error";
    default:
      return "Unknown";
  }
}

export function stateClassName(state: VoiceWaveHudState): string {
  switch (state) {
    case "idle":
      return "bg-surface text-ink-strong border border-edge";
    case "listening":
      return "bg-ink text-ink-contrast";
    case "transcribing":
      return "bg-label text-ink-contrast";
    case "inserted":
      return "bg-ink-hover text-ink-contrast";
    case "error":
      return "bg-status-danger-bg text-status-danger-text";
    default:
      return "bg-inset text-sub";
  }
}
