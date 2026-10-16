import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ArrowRight, Check, Mic, Sparkles } from "lucide-react";

import {
  canUseTauri,
  getPillQuickControlsSnapshot,
  listenVoicewaveMicLevel,
  openMicrophonePrivacySettings,
  setMicLevelForwarding,
  startMicLevelMonitor,
  stopMicLevelMonitor
} from "../lib/tauri";
import type {
  ModelCatalogItem,
  ModelStatus,
  SpeechEngineStatus,
  VoiceWaveSnapshot
} from "../types/voicewave";
import { ModelDownloadProgress } from "./ModelDownloadProgress";

type OnboardingStep = "welcome" | "mic" | "speak" | "done";

const STEP_ORDER: OnboardingStep[] = ["welcome", "mic", "speak", "done"];
const METER_BAR_COUNT = 30;
const METER_RADIUS = 94;
/** Mic-level (RMS-ish, 0..~0.3 for speech) above this counts as "heard". */
const HEARD_LEVEL = 0.045;
/** Cumulative ms above HEARD_LEVEL before the mic check passes. */
const HEARD_HOLD_MS = 600;

interface OnboardingProps {
  /** This PC's two primary rows (Small, Large Turbo) from the per-machine catalog. */
  catalog: ModelCatalogItem[];
  /** Plan 019 engine state: unsupported CPUs, and the one-time graphics card check. */
  speechEngine: SpeechEngineStatus | null;
  statuses: Record<string, ModelStatus>;
  hasInstalledModel: boolean;
  /** Resolves true once the model is installed (see useVoiceWave). */
  installModel: (modelId: string) => Promise<boolean>;
  cancelModelInstall: (modelId: string) => Promise<void>;
  makeModelActive: (modelId: string) => Promise<void>;
  /** Mic step device picker; same list and setting as Settings > Audio. */
  inputDevices: string[];
  inputDevice: string | null;
  setInputDevice: (deviceName: string | null) => Promise<void>;
  hotkeyLabel: string;
  snapshot: VoiceWaveSnapshot;
  /** Anonymous usage count, on by default. Disclosed on the first screen so
   * it is seen before the rehearsal dictation could send anything; changes
   * persist immediately. */
  usageOptIn: boolean;
  onUsageOptInChange: (enabled: boolean) => void;
  onComplete: () => void;
  /** Test/host override. Desktop normally resolves the internal rollout flag
   * from the native snapshot so onboarding never advertises a disabled card. */
  quickControlsAvailable?: boolean;
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

/** Canvas `fillStyle` needs resolved colors, so these stay literal — they are
 * the brand blue family, which is theme-invariant by design (plan 014). */
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
 * (no React re-renders) from live mic-level events at ~30 fps. `onError`
 * reports a microphone that could not be opened (the monitor emits that as a
 * mic-level event, not as a failed command). */
function MicMeter({
  heard,
  onHeard,
  onError
}: {
  heard: boolean;
  onHeard: () => void;
  onError: (message: string) => void;
}) {
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

    // Listen before starting the monitor: a mic that fails to open reports
    // it in the very first event. Cleanup can land mid-setup (fast Continue,
    // StrictMode remount): release a listener that resolves late and bail
    // before starting the monitor.
    void (async () => {
      const fn = await listenVoicewaveMicLevel((event) => {
        levelRef.current = event.error ? 0 : event.level;
        if (event.error) {
          onError(event.error);
        }
      });
      if (!active) {
        fn();
        return;
      }
      unlisten = fn;
      try {
        await setMicLevelForwarding(true);
        if (!active) {
          return;
        }
        await startMicLevelMonitor();
      } catch (err) {
        onError(err instanceof Error ? err.message : String(err));
      }
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
  }, [onError, onHeard]);

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

/** Shown while the one-time graphics card check runs (driver shader compile,
 * 25-41 s measured). Dictation already works meanwhile, on the processor. */
export function GpuPreparingNote({ className = "" }: { className?: string }) {
  return (
    <div className={`flex items-start gap-3 rounded-2xl border border-edge bg-inset px-4 py-3 ${className}`} role="status">
      <span className="vw-gpu-prep-dot mt-1.5" aria-hidden="true" />
      <div>
        <p className="text-sm font-semibold text-ink-strong">
          Getting your graphics card ready (one time, up to a minute)
        </p>
        <p className="mt-0.5 text-xs text-hint">
          Dictation already works. It runs on your processor until this finishes, then gets much faster.
        </p>
      </div>
    </div>
  );
}

export function Onboarding({
  catalog,
  speechEngine,
  statuses,
  hasInstalledModel,
  installModel,
  cancelModelInstall,
  makeModelActive,
  inputDevices,
  inputDevice,
  setInputDevice,
  hotkeyLabel,
  snapshot,
  usageOptIn,
  onUsageOptInChange,
  onComplete,
  quickControlsAvailable: quickControlsAvailableOverride
}: OnboardingProps) {
  const [step, setStep] = useState<OnboardingStep>("welcome");
  const [modelChoice, setModelChoice] = useState(
    () => catalog.find((row) => row.recommended)?.modelId ?? catalog[0]?.modelId ?? "fw-small.en"
  );
  const modelPickedRef = useRef(false);
  // A ref, not state: Retry calls startDownload in the same tick the previous
  // attempt settled, and a state guard read through a stale closure made the
  // first Retry click a no-op.
  const downloadInFlightRef = useRef(false);
  const [micHeard, setMicHeard] = useState(false);
  const [micHintVisible, setMicHintVisible] = useState(false);
  const [micError, setMicError] = useState<string | null>(null);
  const [spokeDone, setSpokeDone] = useState(false);
  const [quickControlsPreviewOpen, setQuickControlsPreviewOpen] = useState(false);
  const [quickControlsAvailable, setQuickControlsAvailable] = useState(
    quickControlsAvailableOverride ?? false
  );
  const shellRef = useRef<HTMLDivElement | null>(null);
  const stageRef = useRef<HTMLDivElement | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  const rehearsalBaselineRef = useRef<string | null>(null);
  const { canvasRef, burst } = useParticleBurst();

  useEffect(() => {
    if (quickControlsAvailableOverride !== undefined) {
      setQuickControlsAvailable(quickControlsAvailableOverride);
      return;
    }
    if (!canUseTauri()) {
      return;
    }
    void getPillQuickControlsSnapshot()
      .then((snapshot) => setQuickControlsAvailable(snapshot.enabled))
      .catch(() => setQuickControlsAvailable(false));
  }, [quickControlsAvailableOverride]);

  // The catalog can arrive after mount; follow its recommendation until the user picks.
  useEffect(() => {
    const recommendedId = catalog.find((row) => row.recommended)?.modelId;
    if (recommendedId && !modelPickedRef.current) {
      setModelChoice(recommendedId);
    }
  }, [catalog]);

  const chooseModel = (modelId: string) => {
    modelPickedRef.current = true;
    setModelChoice(modelId);
  };

  const status = statuses[modelChoice];
  const modelReady =
    hasInstalledModel || status?.installed === true || status?.state === "installed";
  const smallRow = catalog.find((row) => row.tier === "small");
  const turboRow = catalog.find((row) => row.tier === "largeTurbo");
  const gpuMachine = speechEngine?.gpuName != null;
  const cpuUnsupported = speechEngine?.cpuSupport === "unsupported";
  const gpuPreparing = speechEngine?.preparing ?? false;
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

  // The download outlives this component: finishing or skipping onboarding
  // mid-download leaves it running, the model gate shows its progress, and
  // this chain still activates the model when it lands.
  const startDownload = useCallback(() => {
    if (modelReady || downloadInFlightRef.current) {
      return;
    }
    downloadInFlightRef.current = true;
    const modelId = modelChoice;
    void (async () => {
      try {
        if (await installModel(modelId)) {
          await makeModelActive(modelId);
        }
      } catch {
        // Surfaced through the model status (state === "failed" or "cancelled") with a retry.
      } finally {
        downloadInFlightRef.current = false;
      }
    })();
  }, [installModel, makeModelActive, modelChoice, modelReady]);

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

  const handleMicError = useCallback((message: string) => {
    setMicError(message);
  }, []);

  // Switching input restarts the level monitor in place (forwarding stays on)
  // rather than remounting the meter, whose async teardown could switch
  // forwarding off after the new monitor started.
  const changeInputDevice = useCallback(
    async (deviceName: string | null) => {
      setMicError(null);
      await setInputDevice(deviceName);
      if (!canUseTauri()) {
        return;
      }
      try {
        await stopMicLevelMonitor();
        await startMicLevelMonitor();
      } catch (err) {
        setMicError(err instanceof Error ? err.message : String(err));
      }
    },
    [setInputDevice]
  );

  // Nudge quiet mics: if nothing has been heard after a while on the mic step,
  // point at Windows input volume and mic privacy (the most common silent
  // failures we see).
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

  // The monitor reports cpal's wording; "no input device" is the one case
  // with a different fix than Windows privacy / another app holding the mic.
  const micErrorCopy = micError
    ? /no input device/i.test(micError)
      ? "No microphone found. Plug one in, or pick another input below."
      : "VoiceWave can't open your microphone. Windows may be blocking desktop apps from using it, or another app is holding it."
    : null;

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
                <h1 className='mt-3 font-["Fraunces"] text-[44px] leading-[1.06] tracking-tight text-ink-strong'>
                  Your voice,
                  <br />
                  everywhere.
                </h1>
                <p className="mt-4 max-w-[26rem] text-[15px] leading-relaxed text-faint">
                  A quick setup, then dictation in every app on this PC.
                  Private, offline, and yours — audio never leaves this machine.
                </p>

                {cpuUnsupported ? (
                  <div className="mt-6 max-w-[30rem] rounded-2xl border border-status-danger-border bg-status-danger-bg px-5 py-4" role="alert">
                    <p className="text-sm font-semibold text-status-danger-text">This PC isn't supported</p>
                    <p className="mt-1 text-xs leading-relaxed text-status-danger-text">
                      VoiceWave's speech engine needs a processor with SSE4.1, which almost every PC since 2008
                      has. This one doesn't, so dictation can't run here.
                    </p>
                  </div>
                ) : !modelReady && smallRow && turboRow && (
                  <div className="mt-6">
                    <p className="text-xs font-semibold uppercase tracking-[0.14em] text-hint">
                      Speech model
                    </p>
                    <div className="vw-seg mt-2 inline-flex" role="group" aria-label="Speech model choice">
                      <button
                        type="button"
                        className={`vw-seg-btn ${modelChoice === smallRow.modelId ? "vw-seg-btn-active" : ""}`}
                        onClick={() => chooseModel(smallRow.modelId)}
                      >
                        {smallRow.recommended ? "Small · recommended" : "Small"}
                      </button>
                      <button
                        type="button"
                        className={`vw-seg-btn ${modelChoice === turboRow.modelId ? "vw-seg-btn-active" : ""}`}
                        onClick={() => chooseModel(turboRow.modelId)}
                      >
                        {turboRow.recommended
                          ? "Large Turbo · recommended"
                          : gpuMachine
                            ? "Large Turbo"
                            : "Large Turbo · slower on this PC"}
                      </button>
                    </div>
                    {catalogRow && (
                      <p className="mt-2 text-xs text-hint">
                        {catalogRow.hardwareNote} · {formatSize(catalogRow.sizeBytes)} · downloads in the background while you finish setup
                      </p>
                    )}
                  </div>
                )}

                <div className="mt-6 flex max-w-[30rem] items-start justify-between gap-6">
                  <div>
                    <p className="vw-set-title">Anonymous usage count · on by default</p>
                    <p className="vw-set-desc">
                      Counts active installs with a random ID, the app version, and a once-a-day marker.
                      Never audio or text. Change it anytime in Settings.
                    </p>
                  </div>
                  <span className="vw-switch">
                    <input
                      type="checkbox"
                      aria-label="Share anonymous usage count"
                      checked={usageOptIn}
                      onChange={(event) => onUsageOptInChange(event.target.checked)}
                    />
                    <span className="vw-switch-track" aria-hidden="true" />
                  </span>
                </div>

                <div className="mt-auto flex items-center gap-3 pt-8">
                  {cpuUnsupported ? (
                    <button type="button" className="vw-btn-secondary" onClick={onComplete}>
                      Close setup
                    </button>
                  ) : (
                    <button type="button" className="vw-btn-primary" onClick={begin}>
                      {modelReady ? "Get started" : "Set up VoiceWave"}
                      <ArrowRight size={16} className="ml-1.5 inline-block" />
                    </button>
                  )}
                </div>
              </div>
            )}

            {step === "mic" && (
              <div className="vw-onb-step flex flex-1 flex-col">
                <p className="vw-onb-kicker">MICROPHONE</p>
                <h2 className='mt-3 font-["Fraunces"] text-[34px] leading-tight tracking-tight text-ink-strong'>
                  Say something.
                </h2>
                <p className="mt-2 text-[15px] text-faint">
                  {micHeard
                    ? "Your microphone sounds great."
                    : "Read this aloud: “The quick brown fox jumps over the lazy dog.”"}
                </p>

                <div className="flex flex-1 items-center justify-center py-4">
                  <MicMeter heard={micHeard} onHeard={handleMicHeard} onError={handleMicError} />
                </div>

                {!micHeard && (micErrorCopy || micHintVisible) && (
                  <p className="text-center text-xs text-status-warn-text" role={micErrorCopy ? "alert" : undefined}>
                    {micErrorCopy ??
                      "Not hearing anything — check that your mic isn't muted, its Windows input volume is up, and Windows lets desktop apps use it."}{" "}
                    <button
                      type="button"
                      className="font-semibold underline underline-offset-2"
                      onClick={() => void openMicrophonePrivacySettings().catch(() => undefined)}
                    >
                      Open microphone privacy settings
                    </button>
                  </p>
                )}

                <div className="mt-auto flex items-center justify-between gap-3 pt-4">
                  <select
                    className="vw-field max-w-[260px]"
                    aria-label="Microphone input device"
                    value={inputDevice ?? ""}
                    onChange={(event) => void changeInputDevice(event.target.value ? event.target.value : null)}
                  >
                    <option value="">Default system input</option>
                    {inputDevices.map((device) => (
                      <option key={device} value={device}>
                        {device}
                      </option>
                    ))}
                  </select>
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
                <h2 className='mt-3 font-["Fraunces"] text-[34px] leading-tight tracking-tight text-ink-strong'>
                  Hold the key. Speak. Let go.
                </h2>

                {modelReady ? (
                  <>
                    <p className="mt-3 flex flex-wrap items-center gap-2 text-[15px] text-faint">
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

                    <p className="mt-3 text-sm text-faint" aria-live="polite">
                      {spokeDone
                        ? "That's the whole trick — it works exactly like this in every app."
                        : listening
                          ? "Listening…"
                          : transcribing
                            ? "Transcribing…"
                            : " "}
                    </p>
                    <p className="mt-1 text-xs text-hint">
                      Longer thought? Double-tap {hotkeyLabel} for hands-free, then pause or press it again.
                    </p>
                    {gpuPreparing && <GpuPreparingNote className="mt-4" />}
                  </>
                ) : (
                  // While the model downloads, teach both gestures so the wait
                  // is useful; the rehearsal replaces this once it lands.
                  <div className="mt-5 flex flex-1 flex-col">
                    <div className="vw-row-list">
                      <div className="flex items-center gap-3 px-4 py-3 text-[15px] text-faint">
                        <kbd className="vw-onb-kbd">{hotkeyLabel}</kbd>
                        <span>Hold to talk, let go to drop the words where you're typing.</span>
                      </div>
                      <div className="flex items-center gap-3 px-4 py-3 text-[15px] text-faint">
                        <kbd className="vw-onb-kbd">{hotkeyLabel}</kbd>
                        <span>Double-tap for hands-free. Pause when you're done, or press it again.</span>
                      </div>
                    </div>
                    <div className="mt-5">
                      <ModelDownloadProgress
                        status={status}
                        onRetry={startDownload}
                        onCancel={() => void cancelModelInstall(modelChoice)}
                      />
                    </div>
                    <p className="mt-3 text-xs text-hint">
                      One-time download. You can finish setup now; it keeps going in the background,
                      and dictation is fully offline once it lands.
                    </p>
                  </div>
                )}

                <div className="mt-auto flex items-center justify-end pt-5">
                  <button
                    type="button"
                    className={spokeDone ? "vw-btn-primary" : "vw-btn-secondary"}
                    onClick={() => setStep("done")}
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
                <h2 className='mt-6 font-["Fraunces"] text-[38px] leading-tight tracking-tight text-ink-strong'>
                  You're set.
                </h2>
                <p className="mt-3 max-w-[24rem] text-[15px] leading-relaxed text-faint">
                  {modelReady
                    ? "VoiceWave works in every app — try your email or notes next."
                    : status?.state === "failed" || status?.state === "cancelled"
                      ? "The speech model download didn't finish. Pick it up on the next screen, then dictation works in every app."
                      : "The speech model is still downloading. Dictation works in every app the moment it lands."}{" "}
                  Say “new line” or “bullet point” to add structure as you speak.
                </p>
                {quickControlsAvailable ? <button
                  type="button"
                  className={`vw-onb-pill-tip${quickControlsPreviewOpen ? " is-open" : ""}`}
                  aria-expanded={quickControlsPreviewOpen}
                  onClick={() => setQuickControlsPreviewOpen((open) => !open)}
                >
                  <span className="vw-onb-pill-tip-visual" aria-hidden="true">
                    <span className="vw-onb-pill-tip-line" />
                    <span className="vw-onb-pill-tip-card">
                      <span>Standard</span>
                      <span>Writing</span>
                      <span>Hands-free</span>
                    </span>
                  </span>
                  <span className="vw-onb-pill-tip-copy">
                    <strong>Quick Controls</strong>
                    <span>Click the bottom pill for profiles and hands-free.</span>
                  </span>
                </button> : null}
                <div className="mt-5 flex flex-wrap items-center justify-center gap-2">
                  <span className="vw-chip">Spoken commands · on</span>
                  <span className="vw-chip">History · kept on this PC</span>
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
                  className="text-xs font-medium text-hint transition-colors hover:text-quiet"
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
