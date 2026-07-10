import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ArrowRight, Check, Mic, Sparkles } from "lucide-react";

import {
  canUseTauri,
  listenVoicewaveMicLevel,
  setMicLevelForwarding,
  startMicLevelMonitor,
  stopMicLevelMonitor
} from "../lib/tauri";
import type { ModelCatalogItem, ModelStatus, VoiceWaveSnapshot } from "../types/voicewave";

type OnboardingStep = "welcome" | "mic" | "speak" | "done";

const STEP_ORDER: OnboardingStep[] = ["welcome", "mic", "speak", "done"];
const METER_BAR_COUNT = 30;
const METER_RADIUS = 94;
/** Mic-level (RMS-ish, 0..~0.3 for speech) above this counts as "heard". */
const HEARD_LEVEL = 0.045;
/** Cumulative ms above HEARD_LEVEL before the mic check passes. */
const HEARD_HOLD_MS = 600;

interface OnboardingProps {
  catalog: ModelCatalogItem[];
  statuses: Record<string, ModelStatus>;
  hasInstalledModel: boolean;
  installModel: (modelId: string) => Promise<void>;
  makeModelActive: (modelId: string) => Promise<void>;
  hotkeyLabel: string;
  snapshot: VoiceWaveSnapshot;
  onComplete: () => void;
}

function prefersReducedMotion(): boolean {
  return (
    typeof window !== "undefined" &&
    typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches
  );
}

/* ------------------------------------------------------------- particles */

interface Particle {
  x: number;
  y: number;
  vx: number;
  vy: number;
  size: number;
  color: string;
  life: number;
  maxLife: number;
  spin: number;
  angle: number;
  rect: boolean;
}

const PARTICLE_COLORS = ["#1B8EFF", "#7ED8FF", "#0A2A8C", "#A7E8FF", "#FFFFFF"];

/** Lightweight canvas confetti scoped to the onboarding card. `burst(x, y)`
 * takes coordinates relative to the wrapping shell. */
function useParticleBurst() {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const particlesRef = useRef<Particle[]>([]);
  const rafRef = useRef<number | null>(null);

  const tick = useCallback(() => {
    const canvas = canvasRef.current;
    const ctx = canvas?.getContext("2d");
    if (!canvas || !ctx) {
      rafRef.current = null;
      return;
    }
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    const alive: Particle[] = [];
    for (const p of particlesRef.current) {
      p.life += 1;
      if (p.life >= p.maxLife) {
        continue;
      }
      p.vy += 0.11;
      p.vx *= 0.986;
      p.vy *= 0.986;
      p.x += p.vx;
      p.y += p.vy;
      p.angle += p.spin;
      const remaining = 1 - p.life / p.maxLife;
      ctx.globalAlpha = Math.min(1, remaining * 2.4);
      ctx.fillStyle = p.color;
      if (p.rect) {
        ctx.save();
        ctx.translate(p.x, p.y);
        ctx.rotate(p.angle);
        ctx.fillRect(-p.size / 2, -p.size / 4, p.size, p.size / 2);
        ctx.restore();
      } else {
        ctx.beginPath();
        ctx.arc(p.x, p.y, p.size / 2, 0, Math.PI * 2);
        ctx.fill();
      }
      alive.push(p);
    }
    ctx.globalAlpha = 1;
    particlesRef.current = alive;
    rafRef.current = alive.length > 0 ? requestAnimationFrame(tick) : null;
  }, []);

  const burst = useCallback(
    (x: number, y: number, count = 90) => {
      const canvas = canvasRef.current;
      if (!canvas || prefersReducedMotion()) {
        return;
      }
      const shell = canvas.parentElement;
      if (shell) {
        const dpr = window.devicePixelRatio || 1;
        const rect = shell.getBoundingClientRect();
        if (canvas.width !== rect.width * dpr || canvas.height !== rect.height * dpr) {
          canvas.width = rect.width * dpr;
          canvas.height = rect.height * dpr;
        }
        const ctx = canvas.getContext("2d");
        ctx?.setTransform(dpr, 0, 0, dpr, 0, 0);
      }
      for (let i = 0; i < count; i += 1) {
        const angle = Math.random() * Math.PI * 2;
        const speed = 1.6 + Math.random() * 5.4;
        particlesRef.current.push({
          x,
          y,
          vx: Math.cos(angle) * speed,
          vy: Math.sin(angle) * speed - 2.4,
          size: 2.5 + Math.random() * 3.5,
          color: PARTICLE_COLORS[Math.floor(Math.random() * PARTICLE_COLORS.length)],
          life: 0,
          maxLife: 55 + Math.random() * 40,
          spin: (Math.random() - 0.5) * 0.3,
          angle: Math.random() * Math.PI,
          rect: Math.random() < 0.35
        });
      }
      if (rafRef.current === null) {
        rafRef.current = requestAnimationFrame(tick);
      }
    },
    [tick]
  );

  useEffect(() => {
    return () => {
      if (rafRef.current !== null) {
        cancelAnimationFrame(rafRef.current);
      }
    };
  }, []);

  return { canvasRef, burst };
}

/* ------------------------------------------------------------- mic meter */

/** Circular voice visualizer: 30 bars around a gradient orb, driven directly
 * (no React re-renders) from live mic-level events at ~30 fps. */
function MicMeter({ heard, onHeard }: { heard: boolean; onHeard: () => void }) {
  const barsRef = useRef<Array<HTMLDivElement | null>>([]);
  const haloRef = useRef<HTMLDivElement | null>(null);
  const levelRef = useRef(0);
  const heardMsRef = useRef(0);
  const firedRef = useRef(false);

  useEffect(() => {
    if (!canUseTauri()) {
      return;
    }
    let unlisten: (() => void) | null = null;
    let raf = 0;
    let lastTime = performance.now();
    let energy = 0;
    let active = true;

    void (async () => {
      try {
        await setMicLevelForwarding(true);
        await startMicLevelMonitor();
      } catch {
        // The hint copy below the meter covers the "we hear nothing" case.
      }
      unlisten = await listenVoicewaveMicLevel((event) => {
        levelRef.current = event.error ? 0 : event.level;
      });
    })();

    const reduced = prefersReducedMotion();
    const frame = (time: number) => {
      if (!active) {
        return;
      }
      const dt = Math.min(64, time - lastTime);
      lastTime = time;
      const target = Math.min(1, levelRef.current * 6);
      energy += (target - energy) * 0.22;

      if (!firedRef.current) {
        if (levelRef.current > HEARD_LEVEL) {
          heardMsRef.current += dt;
          if (heardMsRef.current >= HEARD_HOLD_MS) {
            firedRef.current = true;
            onHeard();
          }
        } else {
          heardMsRef.current = Math.max(0, heardMsRef.current - dt * 0.5);
        }
      }

      const t = time / 1000;
      for (let i = 0; i < METER_BAR_COUNT; i += 1) {
        const bar = barsRef.current[i];
        if (!bar) {
          continue;
        }
        const character = 0.35 + 0.65 * Math.abs(Math.sin(i * 2.399));
        const wobble = reduced ? 0 : Math.sin(t * 4.2 + i * 0.72) * 7 * energy;
        const height = 10 + energy * 42 * character + wobble;
        bar.style.height = `${Math.max(6, height)}px`;
        bar.style.opacity = `${0.45 + energy * 0.55}`;
      }
      const halo = haloRef.current;
      if (halo) {
        halo.style.transform = `translate(-50%, -50%) scale(${1 + energy * 1.9})`;
        halo.style.opacity = `${0.3 + energy * 0.7}`;
      }
      raf = requestAnimationFrame(frame);
    };
    raf = requestAnimationFrame(frame);

    return () => {
      active = false;
      cancelAnimationFrame(raf);
      if (unlisten) {
        unlisten();
      }
      void (async () => {
        try {
          await stopMicLevelMonitor();
          await setMicLevelForwarding(false);
        } catch {
          // Best effort; the flag resets with the app.
        }
      })();
    };
  }, [onHeard]);

  return (
    <div className="vw-onb-meter" role="img" aria-label="Live microphone level">
      <div ref={haloRef} className="vw-onb-meter-halo" />
      {Array.from({ length: METER_BAR_COUNT }, (_, i) => (
        <div
          key={i}
          ref={(node) => {
            barsRef.current[i] = node;
          }}
          className="vw-onb-meter-bar"
          style={{
            transform: `rotate(${(360 / METER_BAR_COUNT) * i}deg) translateY(-${METER_RADIUS}px)`
          }}
        />
      ))}
      <div className="vw-onb-meter-orb">
        {heard ? <Check size={34} strokeWidth={3} className="vw-onb-tick" /> : <Mic size={30} />}
      </div>
    </div>
  );
}

/* ------------------------------------------------------------ onboarding */

export function Onboarding({
  catalog,
  statuses,
  hasInstalledModel,
  installModel,
  makeModelActive,
  hotkeyLabel,
  snapshot,
  onComplete
}: OnboardingProps) {
  const [step, setStep] = useState<OnboardingStep>("welcome");
  const [modelChoice, setModelChoice] = useState("fw-small.en");
  const [downloadStarted, setDownloadStarted] = useState(false);
  const [micHeard, setMicHeard] = useState(false);
  const [micHintVisible, setMicHintVisible] = useState(false);
  const [spokeDone, setSpokeDone] = useState(false);
  const shellRef = useRef<HTMLDivElement | null>(null);
  const stageRef = useRef<HTMLDivElement | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  const rehearsalBaselineRef = useRef<string | null>(null);
  const { canvasRef, burst } = useParticleBurst();

  const status = statuses[modelChoice];
  const modelReady =
    hasInstalledModel || status?.installed === true || status?.state === "installed";
  const _downloading = !modelReady && (status?.state === "downloading" || downloadStarted);
  const downloadFailed = !modelReady && status?.state === "failed";
  const progress = modelReady ? 100 : Math.max(0, Math.min(100, status?.progress ?? 0));
  const catalogRow = catalog.find((row) => row.modelId === modelChoice);

  const motes = useMemo(
    () =>
      Array.from({ length: 14 }, (_, i) => ({
        id: i,
        style: {
          "--mx": `${6 + Math.random() * 88}%`,
          "--my": `${18 + Math.random() * 72}%`,
          "--ms": `${2 + Math.random() * 3}px`,
          "--md": `${7 + Math.random() * 9}s`,
          "--mdel": `${Math.random() * 8}s`,
          "--mdx": `${(Math.random() - 0.5) * 60}px`
        } as React.CSSProperties
      })),
    []
  );

  const startDownload = useCallback(() => {
    if (modelReady || downloadStarted) {
      return;
    }
    setDownloadStarted(true);
    void (async () => {
      try {
        await installModel(modelChoice);
        await makeModelActive(modelChoice);
      } catch {
        // Surfaced through the model status (state === "failed") with a retry.
        setDownloadStarted(false);
      }
    })();
  }, [downloadStarted, installModel, makeModelActive, modelChoice, modelReady]);

  const begin = useCallback(() => {
    startDownload();
    setStep("mic");
  }, [startDownload]);

  const handleMicHeard = useCallback(() => {
    setMicHeard(true);
    const shell = shellRef.current;
    if (shell) {
      const rect = shell.getBoundingClientRect();
      burst(rect.width / 2, rect.height / 2 - 20, 60);
    }
  }, [burst]);

  // Nudge quiet mics: if nothing has been heard after a while on the mic step,
  // point at Windows input volume (the most common silent failure we see).
  useEffect(() => {
    if (step !== "mic" || micHeard) {
      setMicHintVisible(false);
      return;
    }
    const handle = window.setTimeout(() => setMicHintVisible(true), 7000);
    return () => window.clearTimeout(handle);
  }, [micHeard, step]);

  // Rehearsal success: the dictation pipeline updates snapshot.lastFinal when
  // a real dictation lands. Insertion types into the focused textarea; if it
  // fell back to the clipboard instead, mirror the text so the moment can't
  // visually fail.
  useEffect(() => {
    if (step !== "speak") {
      return;
    }
    if (rehearsalBaselineRef.current === null) {
      rehearsalBaselineRef.current = snapshot.lastFinal ?? "";
      return;
    }
    const final = snapshot.lastFinal ?? "";
    if (spokeDone || final.length === 0 || final === rehearsalBaselineRef.current) {
      return;
    }
    const textarea = textareaRef.current;
    if (textarea && textarea.value.trim().length === 0) {
      textarea.value = final;
    }
    setSpokeDone(true);
    stageRef.current?.classList.remove("vw-onb-stage-flash");
    // Force a reflow so re-adding the class replays the flash animation.
    void stageRef.current?.offsetWidth;
    stageRef.current?.classList.add("vw-onb-stage-flash");
    const shell = shellRef.current;
    const stage = stageRef.current;
    if (shell && stage) {
      const shellRect = shell.getBoundingClientRect();
      const stageRect = stage.getBoundingClientRect();
      burst(
        stageRect.left - shellRect.left + stageRect.width / 2,
        stageRect.top - shellRect.top + stageRect.height / 2,
        110
      );
    }
  }, [burst, snapshot.lastFinal, spokeDone, step]);

  const finishBurstFiredRef = useRef(false);
  useEffect(() => {
    if (step !== "done" || finishBurstFiredRef.current) {
      return;
    }
    finishBurstFiredRef.current = true;
    const shell = shellRef.current;
    if (shell) {
      const rect = shell.getBoundingClientRect();
      const handle = window.setTimeout(() => burst(rect.width / 2, rect.height / 3, 130), 240);
      return () => window.clearTimeout(handle);
    }
  }, [burst, step]);

  const listening = snapshot.state === "listening";
  const transcribing = snapshot.state === "transcribing";
  const stepIndex = STEP_ORDER.indexOf(step);

  const formatSize = (bytes?: number) =>
    bytes ? `${Math.round(bytes / (1024 * 1024))} MB` : "";

  return (
    <div className="vw-onb-backdrop" role="dialog" aria-modal="true" aria-label="Welcome to VoiceWave">
      {step === "welcome" &&
        motes.map((mote) => <span key={mote.id} className="vw-onb-mote" style={mote.style} />)}

      <div ref={shellRef} className="vw-onb-shell">
        <div className="vw-ring-shell vw-ring-shell-lg">
          <div className="vw-ring-inner vw-onb-card-inner">
            {step === "welcome" && (
              <div className="vw-onb-step flex flex-1 flex-col">
                <p className="vw-onb-kicker">VOICEWAVE</p>
                <h1 className='mt-3 font-["Fraunces"] text-[44px] leading-[1.06] tracking-tight text-[#09090B]'>
                  Your voice,
                  <br />
                  everywhere.
                </h1>
                <p className="mt-4 max-w-[26rem] text-[15px] leading-relaxed text-[#71717A]">
                  A minute of setup, then dictation in every app on this PC.
                  Private, offline, and yours — audio never leaves this machine.
                </p>

                {!modelReady && (
                  <div className="mt-6">
                    <p className="text-xs font-semibold uppercase tracking-[0.14em] text-[#A1A1AA]">
                      Speech model
                    </p>
                    <div className="vw-seg mt-2 inline-flex" role="group" aria-label="Speech model choice">
                      <button
                        type="button"
                        className={`vw-seg-btn ${modelChoice === "fw-small.en" ? "vw-seg-btn-active" : ""}`}
                        onClick={() => setModelChoice("fw-small.en")}
                      >
                        Small · recommended
                      </button>
                      <button
                        type="button"
                        className={`vw-seg-btn ${modelChoice === "fw-large-v3-turbo" ? "vw-seg-btn-active" : ""}`}
                        onClick={() => setModelChoice("fw-large-v3-turbo")}
                      >
                        Large Turbo · max accuracy
                      </button>
                    </div>
                    {catalogRow && (
                      <p className="mt-2 text-xs text-[#A1A1AA]">
                        {catalogRow.displayName} · {formatSize(catalogRow.sizeBytes)} · downloads in the background while you finish setup
                      </p>
                    )}
                  </div>
                )}

                <div className="mt-auto flex items-center gap-3 pt-8">
                  <button type="button" className="vw-btn-primary" onClick={begin}>
                    {modelReady ? "Get started" : "Set up VoiceWave"}
                    <ArrowRight size={16} className="ml-1.5 inline-block" />
                  </button>
                </div>
              </div>
            )}

            {step === "mic" && (
              <div className="vw-onb-step flex flex-1 flex-col">
                <p className="vw-onb-kicker">MICROPHONE</p>
                <h2 className='mt-3 font-["Fraunces"] text-[34px] leading-tight tracking-tight text-[#09090B]'>
                  Say something.
                </h2>
                <p className="mt-2 text-[15px] text-[#71717A]">
                  {micHeard
                    ? "Your microphone sounds great."
                    : "Read this aloud: “The quick brown fox jumps over the lazy dog.”"}
                </p>

                <div className="flex flex-1 items-center justify-center py-4">
                  <MicMeter heard={micHeard} onHeard={handleMicHeard} />
                </div>

                {!micHeard && micHintVisible && (
                  <p className="text-center text-xs text-[#B45309]">
                    Not hearing anything — check that your mic isn't muted and its Windows input volume is up.
                  </p>
                )}

                <div className="mt-auto flex items-center justify-end pt-4">
                  <button
                    type="button"
                    className={micHeard ? "vw-btn-primary" : "vw-btn-secondary"}
                    onClick={() => setStep("speak")}
                  >
                    Continue
                    <ArrowRight size={16} className="ml-1.5 inline-block" />
                  </button>
                </div>
              </div>
            )}

            {step === "speak" && (
              <div className="vw-onb-step flex flex-1 flex-col">
                <p className="vw-onb-kicker">THE ONE GESTURE</p>
                <h2 className='mt-3 font-["Fraunces"] text-[34px] leading-tight tracking-tight text-[#09090B]'>
                  Hold the key. Speak. Let go.
                </h2>

                {modelReady ? (
                  <>
                    <p className="mt-3 flex flex-wrap items-center gap-2 text-[15px] text-[#71717A]">
                      <span>Hold</span>
                      <kbd className={`vw-onb-kbd ${listening ? "vw-onb-kbd-pressed" : ""}`}>
                        {hotkeyLabel}
                      </kbd>
                      <span>and say what you did this morning.</span>
                    </p>

                    <div
                      ref={stageRef}
                      className={`vw-onb-stage mt-5 ${listening || transcribing ? "vw-onb-stage-live" : ""}`}
                    >
                      <textarea
                        ref={textareaRef}
                        autoFocus
                        aria-label="Dictation playground"
                        placeholder={
                          listening
                            ? "Listening…"
                            : transcribing
                              ? "Transcribing…"
                              : "Your words will land here."
                        }
                      />
                    </div>

                    <p className="mt-3 text-sm text-[#71717A]" aria-live="polite">
                      {spokeDone
                        ? "That's the whole trick — it works exactly like this in every app."
                        : listening
                          ? "Listening…"
                          : transcribing
                            ? "Transcribing…"
                            : " "}
                    </p>
                  </>
                ) : (
                  <div className="mt-6 flex flex-1 flex-col justify-center">
                    <div className="rounded-2xl border border-[#E4E4E7] bg-[#FAFAFA] px-5 py-5">
                      <div className="flex items-center justify-between gap-3">
                        <p className="text-sm font-semibold text-[#09090B]">
                          {downloadFailed
                            ? "The model download hit a snag."
                            : "Finishing the speech model download…"}
                        </p>
                        {downloadFailed ? (
                          <button
                            type="button"
                            className="vw-btn-primary vw-btn-sm"
                            onClick={() => {
                              setDownloadStarted(false);
                              startDownload();
                            }}
                          >
                            Retry
                          </button>
                        ) : (
                          <span className="text-sm tabular-nums text-[#0A2A8C]">{progress}%</span>
                        )}
                      </div>
                      {!downloadFailed && (
                        <div className="vw-onb-progress mt-3">
                          <div className="vw-onb-progress-fill" style={{ width: `${Math.max(4, progress)}%` }} />
                        </div>
                      )}
                      {downloadFailed && status?.message && (
                        <p className="mt-2 text-xs text-[#A94444]">{status.message}</p>
                      )}
                    </div>
                    <p className="mt-3 text-xs text-[#A1A1AA]">
                      This is the one-time download — dictation is fully offline after it lands.
                    </p>
                  </div>
                )}

                <div className="mt-auto flex items-center justify-end pt-5">
                  <button
                    type="button"
                    className={spokeDone ? "vw-btn-primary" : "vw-btn-secondary"}
                    onClick={() => setStep("done")}
                    disabled={!modelReady && !downloadFailed}
                  >
                    Continue
                    <ArrowRight size={16} className="ml-1.5 inline-block" />
                  </button>
                </div>
              </div>
            )}

            {step === "done" && (
              <div className="vw-onb-step flex flex-1 flex-col items-center justify-center text-center">
                <div className="vw-onb-done-orb">
                  <Check size={40} strokeWidth={3} />
                </div>
                <h2 className='mt-6 font-["Fraunces"] text-[38px] leading-tight tracking-tight text-[#09090B]'>
                  You're set.
                </h2>
                <p className="mt-3 max-w-[24rem] text-[15px] leading-relaxed text-[#71717A]">
                  VoiceWave works in every app — try your email or notes next.
                  Say “new line” or “bullet point” to add structure as you speak.
                </p>
                <div className="mt-5 flex flex-wrap items-center justify-center gap-2">
                  <span className="vw-chip">Spoken commands · on</span>
                  <span className="vw-chip">History · kept 30 days, on-device</span>
                  <span className="vw-chip">
                    <Sparkles size={12} className="mr-1 inline-block" />
                    AI polish · waiting in Settings
                  </span>
                </div>
                <button type="button" className="vw-btn-primary mt-8" onClick={onComplete}>
                  Start dictating
                </button>
              </div>
            )}

            {step !== "done" && (
              <div className="mt-6 flex items-center justify-between">
                <div className="vw-onb-dots" aria-hidden>
                  {STEP_ORDER.slice(0, 3).map((name, index) => (
                    <span
                      key={name}
                      className={`vw-onb-dot ${
                        index === stepIndex
                          ? "vw-onb-dot-active"
                          : index < stepIndex
                            ? "vw-onb-dot-done"
                            : ""
                      }`}
                    />
                  ))}
                </div>
                <button
                  type="button"
                  className="text-xs font-medium text-[#A1A1AA] transition-colors hover:text-[#52525B]"
                  onClick={onComplete}
                >
                  Skip setup
                </button>
              </div>
            )}
          </div>
        </div>

        <canvas ref={canvasRef} className="vw-onb-particles" />
      </div>
    </div>
  );
}
