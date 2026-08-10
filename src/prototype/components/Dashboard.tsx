import type React from "react";
import { Cpu, Mic, Pause, Zap } from "lucide-react";
import type { DictationState, ThemeConfig } from "../types";
import { useEffect, useRef, useState } from "react";

interface DashboardProps {
  theme: ThemeConfig;
  status: DictationState;
  onPressStart: () => void;
  onPressEnd: () => void;
  currentModel: string;
  partialTranscript: string | null;
  finalTranscript: string | null;
  pushToTalkHotkey: string;
  userName?: string;
  isPro?: boolean;
  recentSentences?: Array<{
    id: string;
    text: string;
    createdAtUtcMs: number;
  }>;
  /** True when the history retention policy is "off" — the empty state then
   * explains why nothing appears instead of implying the user hasn't dictated. */
  historyOff?: boolean;
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
  userName,
  recentSentences = [],
  historyOff = false
}) => {
  const { colors, typography, shapes } = theme;
  const activePointerIdRef = useRef<number | null>(null);
  const keyboardPressActiveRef = useRef(false);
  const [visualStatus, setVisualStatus] = useState<DictationState>(status);
  useEffect(() => {
    if (status !== "inserted") {
      setVisualStatus(status);
      return;
    }
    setVisualStatus("inserted");
    const timeoutId = window.setTimeout(() => {
      setVisualStatus("idle");
    }, 1250);
    return () => {
      window.clearTimeout(timeoutId);
    };
  }, [status]);

  const isRecording = visualStatus === "listening" || visualStatus === "transcribing";
  const idleHint = finalTranscript ?? partialTranscript ?? `Hold ${pushToTalkHotkey} to start capturing`;
  const statusMeta = STATUS_META[visualStatus];
  const stateClass = `vw-home-state-${visualStatus}`;

  const transcriptRows = recentSentences.map((session, index) => ({
    id: session.id,
    time: new Date(session.createdAtUtcMs).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" }),
    text: session.text,
    latest: index === 0
  }));

  return (
    <div className="max-w-6xl mx-auto space-y-8 pb-16">
      <section className="flex flex-col md:flex-row md:items-end justify-between gap-6">
        <div className="pt-2">
          <h1 className={`${typography.fontDisplay} text-5xl mb-2 ${colors.textPrimary} tracking-tight`}>
            {userName ? `Welcome back, ${userName}.` : "Welcome to VoiceWave."}
          </h1>
          <p className={`${colors.textSecondary} text-lg font-light opacity-80`}>System is local and secure. Ready to transcribe.</p>
        </div>

        <div className="flex items-center gap-2 pb-1 text-sm text-faint">
          <span>Hold</span>
          <kbd className="rounded-lg border border-edge bg-surface px-2 py-1 font-sans text-xs font-semibold text-ink-strong shadow-[var(--vw-shadow-card)]">
            {pushToTalkHotkey}
          </kbd>
          <span>to dictate anywhere</span>
        </div>
      </section>

      <div className="space-y-6">
        <section>
          <div className="grid gap-4 md:grid-cols-[1fr_320px]">
            <div className={`vw-ring-shell vw-ring-shell-lg ${shapes.radius}`}>
              <div
                className={`
                  p-8 flex flex-col md:flex-row md:items-center md:justify-between gap-5
                  ${colors.surface} ${shapes.radius}
                  vw-ring-inner vw-home-state-card ${stateClass}
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
                    if (activePointerIdRef.current !== null) {
                      return;
                    }
                    activePointerIdRef.current = event.pointerId;
                    if (event.currentTarget.setPointerCapture) {
                      event.currentTarget.setPointerCapture(event.pointerId);
                    }
                    onPressStart();
                  }}
                  onPointerUp={(event) => {
                    event.preventDefault();
                    if (activePointerIdRef.current !== event.pointerId) {
                      return;
                    }
                    if (
                      event.currentTarget.hasPointerCapture &&
                      event.currentTarget.hasPointerCapture(event.pointerId)
                    ) {
                      event.currentTarget.releasePointerCapture(event.pointerId);
                    }
                    activePointerIdRef.current = null;
                    onPressEnd();
                  }}
                  onPointerCancel={(event) => {
                    if (activePointerIdRef.current !== event.pointerId) {
                      return;
                    }
                    activePointerIdRef.current = null;
                    onPressEnd();
                  }}
                  onLostPointerCapture={() => {
                    if (activePointerIdRef.current === null) {
                      return;
                    }
                    activePointerIdRef.current = null;
                    onPressEnd();
                  }}
                  onKeyDown={(event) => {
                    if (event.key === " " || event.key === "Enter") {
                      if (event.repeat || keyboardPressActiveRef.current) {
                        return;
                      }
                      event.preventDefault();
                      keyboardPressActiveRef.current = true;
                      onPressStart();
                    }
                  }}
                  onKeyUp={(event) => {
                    if (event.key === " " || event.key === "Enter") {
                      if (!keyboardPressActiveRef.current) {
                        return;
                      }
                      event.preventDefault();
                      keyboardPressActiveRef.current = false;
                      onPressEnd();
                    }
                  }}
                  className={`
                    vw-home-mic-button h-20 w-20 shrink-0 flex items-center justify-center transition-all duration-300
                    ${isRecording ? colors.recording : colors.accent}
                    ${colors.accentFg} ${shapes.buttonShape}
                    vw-home-mic-state ${stateClass}
                    ${isRecording ? "vw-home-mic-button-active" : ""}
                  `}
                  type="button"
                  aria-label="Hold to dictate"
                >
                  {isRecording ? <Pause size={28} fill="currentColor" /> : <Mic size={28} />}
                </button>
              </div>
            </div>

            <div className="space-y-3">
              <div className="vw-ring-shell vw-ring-shell-sm rounded-3xl">
                <div className={`rounded-3xl ${colors.surface} px-4 py-3 vw-ring-inner vw-home-secondary-card`}>
                  <div className="flex items-center justify-between gap-3">
                    <div className="flex items-center gap-3">
                      <div
                        className="h-9 w-9 rounded-xl flex items-center justify-center"
                        style={{ backgroundImage: colors.accentGradientSoft }}
                      >
                        <Cpu size={16} style={{ color: colors.accentBlue }} />
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

              <div className="vw-ring-shell vw-ring-shell-sm rounded-3xl">
                <div className={`rounded-3xl ${colors.surface} px-4 py-3 vw-ring-inner vw-home-secondary-card`}>
                  <div className="flex items-center justify-between gap-3">
                    <div className="flex items-center gap-3">
                      <div
                        className="h-9 w-9 rounded-xl flex items-center justify-center"
                        style={{ backgroundImage: colors.accentGradientSoft }}
                      >
                        <Zap size={16} style={{ color: colors.accentCyan }} />
                      </div>
                      <div>
                        <p className="vw-section-heading text-sm font-semibold text-[color:var(--vw-color-text-primary)] leading-none">Mode</p>
                        <p className="mt-1 text-[11px] tracking-[0.14em] text-[color:var(--vw-color-text-muted)]">{statusMeta.modeLabel}</p>
                      </div>
                    </div>
                    <span className={`rounded-xl border px-2 py-0.5 text-[10px] font-semibold vw-home-mode-chip ${stateClass}`}>
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
              ${isRecording ? "bg-black" : `${colors.surface} border ${colors.surfaceBorder} shadow-[0_1px_2px_rgba(9,9,11,0.04)]`}
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
                <p className="text-quiet text-sm md:text-base leading-relaxed text-left md:text-center max-w-[56rem]">
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
          {transcriptRows.length === 0 && (
            <div className="px-6 py-8 text-center">
              <p className="text-sm text-faint">
                {historyOff
                  ? "History is off, so dictations aren't kept. Turn it on in the History tab to see them here."
                  : "Your recent dictations will appear here."}
              </p>
            </div>
          )}
          {transcriptRows.map((row, index) => (
            <div
              key={row.id}
              className={`grid grid-cols-[110px_1fr] gap-0 ${
                index !== transcriptRows.length - 1 ? `border-b border-hairline` : ""
              } ${row.latest ? "vw-home-row-latest" : ""}`}
            >
              <div className="px-6 py-4 text-sm text-faint">{row.time}</div>
              <div
                className={`px-6 py-4 text-base leading-relaxed ${
                  row.latest
                    ? "text-ink-strong font-medium"
                    : "text-ink"
                }`}
              >
                {row.text}
              </div>
            </div>
          ))}
          {transcriptRows.length === 0 && (
            <div className="px-6 py-10 text-center">
              <p className="text-sm font-medium text-ink-strong">Nothing dictated yet today</p>
              <p className="mt-1 text-sm text-faint">
                Hold {pushToTalkHotkey} in any app and your words will land here.
              </p>
            </div>
          )}
        </div>
      </section>
    </div>
  );
};
