import type React from "react";
import { Cpu, Mic, Pause, Zap } from "lucide-react";
import { MOCK_SESSIONS } from "../constants";
import type { DictationState, ThemeConfig } from "../types";

interface DashboardProps {
  theme: ThemeConfig;
  status: DictationState;
  onPressStart: () => void;
  onPressEnd: () => void;
  currentModel: string;
  partialTranscript: string | null;
  finalTranscript: string | null;
  pushToTalkHotkey: string;
  isPro?: boolean;
  recentSentences?: Array<{
    id: string;
    text: string;
    createdAtUtcMs: number;
  }>;
}

const WAVE_BARS = [18, 34, 26, 44, 30, 50, 22, 42, 28, 36, 24, 40];

const STATUS_META: Record<
  DictationState,
  { title: string; hint: string; badge: string; modeLabel: string }
> = {
  idle: {
    title: "Start Dictation",
    hint: "Press and hold to talk. Release to transcribe.",
    badge: "Ready",
    modeLabel: "PUSH TO TALK"
  },
  listening: {
    title: "Listening...",
    hint: "Live capture active.",
    badge: "Live",
    modeLabel: "PUSH TO TALK"
  },
  transcribing: {
    title: "Transcribing...",
    hint: "Local decode in progress.",
    badge: "Decoding",
    modeLabel: "AUTO"
  },
  inserted: {
    title: "Inserted",
    hint: "Delivered to active app.",
    badge: "Inserted",
    modeLabel: "AUTO"
  },
  error: {
    title: "Recovered",
    hint: "Saved to history and clipboard.",
    badge: "Fallback",
    modeLabel: "AUTO"
  }
};

function modelLabel(modelId: string): string {
  if (modelId === "fw-small.en" || modelId === "fw-small-en") {
    return "FW SMALL.EN";
  }
  if (modelId === "fw-large-v3") {
    return "FW LARGE-V3";
  }
  return modelId.toUpperCase();
}

export const Dashboard: React.FC<DashboardProps> = ({
  theme,
  status,
  onPressStart,
  onPressEnd,
  currentModel,
  partialTranscript,
  finalTranscript,
  pushToTalkHotkey,
  isPro = false,
  recentSentences = []
}) => {
  const { colors, typography, shapes } = theme;
  const isRecording = status === "listening" || status === "transcribing";
  const proIconGradient = "linear-gradient(135deg, rgba(10,42,140,0.1) 8%, rgba(27,142,255,0.2) 54%, rgba(126,216,255,0.26) 84%, rgba(167,232,255,0.2) 100%)";
  const idleHint = finalTranscript ?? partialTranscript ?? `Hold ${pushToTalkHotkey} to start capturing`;
  const hasFinal = Boolean(finalTranscript && finalTranscript.trim().length > 0);
  const nowLabel = new Date().toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
  const statusMeta = STATUS_META[status];
  const stateClass = `vw-home-state-${status}`;

  const fallbackRows =
    recentSentences.length === 0
      ? MOCK_SESSIONS.map((session) => ({
          id: session.id,
          time: session.date,
          text: session.preview,
          latest: false
        }))
      : [];
  const syncedRows = recentSentences.map((session) => ({
    id: session.id,
    time: new Date(session.createdAtUtcMs).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" }),
    text: session.text,
    latest: false
  }));
  const transcriptRows = [
    ...(hasFinal ? [{ id: "latest", time: nowLabel, text: finalTranscript ?? "", latest: true }] : []),
    ...syncedRows,
    ...fallbackRows
  ];

  return (
    <div className="max-w-6xl mx-auto space-y-8 pb-16">
      <section className="flex flex-col md:flex-row md:items-start justify-between gap-6">
        <div className="pt-2">
          <h1 className={`${typography.fontDisplay} text-5xl mb-2 ${colors.textPrimary} tracking-tight`}>Good morning, Rishi.</h1>
          <p className={`${colors.textSecondary} text-lg font-light opacity-80`}>System is local and secure. Ready to transcribe.</p>
          {isPro && (
            <div className="mt-3">
              <span className="vw-home-pro-title-chip">Pro Workspace Active</span>
            </div>
          )}
        </div>

        <div
          className={`flex items-center justify-end gap-6 px-6 py-3 mt-1 ${colors.surface} border ${colors.surfaceBorder} rounded-3xl shadow-sm vw-home-secondary-metrics ${
            isPro ? "vw-home-pro-metrics" : ""
          }`}
        >
          <div className="flex flex-col items-end">
            <span className="text-[10px] uppercase font-bold tracking-wider text-gray-400">Streak</span>
            <span className={`text-lg font-bold ${colors.textPrimary} leading-none`}>
              12<span className="text-xs font-medium opacity-50 ml-0.5">days</span>
            </span>
          </div>
          <div className="w-px h-8 bg-[rgba(9,9,11,0.12)]" />
          <div className="flex flex-col items-end">
            <span className="text-[10px] uppercase font-bold tracking-wider text-gray-400">Today</span>
            <span className={`text-lg font-bold ${colors.textPrimary} leading-none`}>
              2.4k<span className="text-xs font-medium opacity-50 ml-0.5">words</span>
            </span>
          </div>
          <div className="w-px h-8 bg-[rgba(9,9,11,0.12)]" />
          <div className="flex flex-col items-end">
            <span className="text-[10px] uppercase font-bold tracking-wider text-gray-400">Accuracy</span>
            <span className="vw-positive-stat text-lg font-bold leading-none">
              99.2<span className="text-xs font-medium opacity-70 ml-0.5">%</span>
            </span>
          </div>
        </div>
      </section>

      <div className="space-y-6">
        <section>
          <div className="grid gap-4 md:grid-cols-[1fr_320px]">
            <div className={isPro ? `vw-ring-shell vw-ring-shell-lg ${shapes.radius}` : ""}>
              <div
                className={`
                  p-8 flex flex-col md:flex-row md:items-center md:justify-between gap-5
                  ${colors.surface} border border-[color:var(--vw-color-border)] ${shapes.radius}
                  vw-home-main-card vw-home-state-card ${stateClass}
                  ${isPro ? "vw-home-pro-panel vw-ring-inner" : ""}
                `}
              >
                <div>
                  <h3 className={`${typography.fontDisplay} text-2xl ${colors.textPrimary} mb-1`}>
                    {statusMeta.title}
                  </h3>
                  <p className={`${colors.textSecondary} text-sm`}>{statusMeta.hint}</p>
                  <div className="mt-2 flex flex-wrap items-center gap-2">
                    <p className="text-xs uppercase tracking-[0.16em] text-[color:var(--vw-color-text-muted)]">Model: {currentModel}</p>
                    <span className={`vw-home-state-badge ${stateClass}`}>{statusMeta.badge}</span>
                  </div>
                </div>

                <button
                  onPointerDown={(event) => {
                    event.preventDefault();
                    if (event.currentTarget.setPointerCapture) {
                      event.currentTarget.setPointerCapture(event.pointerId);
                    }
                    onPressStart();
                  }}
                  onPointerUp={(event) => {
                    event.preventDefault();
                    if (
                      event.currentTarget.hasPointerCapture &&
                      event.currentTarget.hasPointerCapture(event.pointerId)
                    ) {
                      event.currentTarget.releasePointerCapture(event.pointerId);
                    }
                    onPressEnd();
                  }}
                  onPointerCancel={() => onPressEnd()}
                  onKeyDown={(event) => {
                    if (event.key === " " || event.key === "Enter") {
                      event.preventDefault();
                      onPressStart();
                    }
                  }}
                  onKeyUp={(event) => {
                    if (event.key === " " || event.key === "Enter") {
                      event.preventDefault();
                      onPressEnd();
                    }
                  }}
                  className={`
                    vw-home-mic-button h-20 w-20 shrink-0 flex items-center justify-center transition-all duration-300
                    ${isRecording ? colors.recording : colors.accent}
                    ${colors.accentFg} ${shapes.buttonShape}
                    vw-home-mic-state ${stateClass}
                    ${isRecording ? "vw-home-mic-button-active" : ""}
                    ${isPro ? "vw-home-pro-mic" : ""}
                  `}
                  type="button"
                  aria-label="Hold to dictate"
                >
                  {isRecording ? <Pause size={28} fill="currentColor" /> : <Mic size={28} />}
                </button>
              </div>
            </div>

            <div className="space-y-3">
              <div className={isPro ? "vw-ring-shell vw-ring-shell-sm rounded-3xl" : ""}>
                <div
                  className={`rounded-3xl border ${colors.surfaceBorder} ${colors.surface} px-4 py-3 shadow-sm vw-home-secondary-card ${
                    isPro ? "vw-home-pro-sidecard vw-ring-inner" : ""
                  }`}
                >
                  <div className="flex items-center justify-between gap-3">
                    <div className="flex items-center gap-3">
                      <div
                        className="h-9 w-9 rounded-xl flex items-center justify-center"
                        style={{ backgroundImage: isPro ? proIconGradient : colors.accentGradientSoft }}
                      >
                        <Cpu size={16} style={isPro ? { color: colors.accentBlue } : undefined} className={!isPro ? "text-[#18181B]" : undefined} />
                      </div>
                      <div>
                        <p className="vw-section-heading text-sm font-semibold text-[color:var(--vw-color-text-primary)] leading-none">Model</p>
                        <p className="mt-1 text-[11px] tracking-[0.14em] text-[color:var(--vw-color-text-muted)]">
                          {modelLabel(currentModel)}
                        </p>
                      </div>
                    </div>
                    <div className={`h-2.5 w-2.5 rounded-full vw-status-dot ${stateClass}`} />
                  </div>
                </div>
              </div>

              <div className={isPro ? "vw-ring-shell vw-ring-shell-sm rounded-3xl" : ""}>
                <div
                  className={`rounded-3xl border ${colors.surfaceBorder} ${colors.surface} px-4 py-3 shadow-sm vw-home-secondary-card ${
                    isPro ? "vw-home-pro-sidecard vw-ring-inner" : ""
                  }`}
                >
                  <div className="flex items-center justify-between gap-3">
                    <div className="flex items-center gap-3">
                      <div
                        className="h-9 w-9 rounded-xl flex items-center justify-center"
                        style={{ backgroundImage: isPro ? proIconGradient : colors.accentGradientSoft }}
                      >
                        <Zap size={16} style={isPro ? { color: colors.accentCyan } : undefined} className={!isPro ? "text-[#18181B]" : undefined} />
                      </div>
                      <div>
                        <p className="vw-section-heading text-sm font-semibold text-[color:var(--vw-color-text-primary)] leading-none">Mode</p>
                        <p className="mt-1 text-[11px] tracking-[0.14em] text-[color:var(--vw-color-text-muted)]">{statusMeta.modeLabel}</p>
                      </div>
                    </div>
                    <span
                      className={`rounded-xl border px-2 py-0.5 text-[10px] font-semibold ${
                        isPro
                          ? "border-[rgba(27,142,255,0.52)] bg-[rgba(27,142,255,0.14)] text-[#18181B]"
                          : `vw-home-mode-chip ${stateClass}`
                      }`}
                    >
                      {statusMeta.badge}
                    </span>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </section>

        <section>
          <div
            className={`
              w-full min-h-24 relative overflow-hidden transition-colors duration-200 vw-home-state-output ${stateClass}
              ${isRecording ? "bg-black shadow-xl" : `${colors.surface} border ${colors.surfaceBorder} shadow-sm`}
              ${isPro && !isRecording ? "vw-home-pro-output" : ""}
              ${isPro && !isRecording ? "vw-home-main-card" : ""}
              ${shapes.radius} flex items-center px-8 py-6
            `}
          >
            <div className="flex-1 flex items-center justify-center">
              {isRecording ? (
                <div className="flex items-center justify-center gap-1 h-8">
                  {WAVE_BARS.map((height, index) => (
                    <div
                      key={index}
                      className="w-1 bg-white rounded-full animate-pulse"
                      style={{
                        height: `${height}%`,
                        animationDelay: `${index * 0.05}s`,
                        animationDuration: "0.8s"
                      }}
                    />
                  ))}
                </div>
              ) : (
                <p className="text-[#52525B] text-sm md:text-base leading-relaxed text-left md:text-center max-w-[56rem]">
                  {idleHint}
                </p>
              )}
            </div>
          </div>
        </section>
      </div>

      <section className="pt-3">
        <p className="vw-section-heading mb-3 text-xs font-semibold tracking-[0.18em] text-[color:var(--vw-color-text-muted)]">TODAY</p>
        <div
          className={`overflow-hidden rounded-3xl border ${colors.surfaceBorder} ${colors.surface} vw-home-transcript-card`}
        >
          {transcriptRows.map((row, index) => (
            <div
              key={row.id}
              className={`grid grid-cols-[110px_1fr] gap-0 ${
                index !== transcriptRows.length - 1 ? `border-b ${colors.surfaceBorder}` : ""
              } ${isPro && row.latest ? "vw-home-row-latest" : ""}`}
            >
              <div className="px-6 py-4 text-sm text-[#71717A]">{row.time}</div>
              <div
                className={`px-6 py-4 text-base leading-relaxed ${
                  row.latest
                    ? "text-[#09090B] font-medium"
                    : "text-[#27272A]"
                }`}
              >
                {row.text}
              </div>
            </div>
          ))}
          {transcriptRows.length === 0 && (
            <div className="px-6 py-8 text-[#71717A]">No transcript results yet.</div>
          )}
        </div>
      </section>
    </div>
  );
};
