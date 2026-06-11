import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  approveDictionaryEntry,
  canUseTauri,
  getDictionaryQueue,
  loadSnapshot,
  rejectDictionaryEntry,
  listenVoicewaveMicLevel,
  listenVoicewaveState,
  setPillReviewMode,
  showMainWindow
} from "../lib/tauri";
import type { DictionaryQueueItem, VoiceWaveHudState } from "../types/voicewave";

type VisualState = "idle" | "listening" | "transcribing" | "inserted" | "error";

function clamp01(value: number): number {
  return Math.max(0, Math.min(1, value));
}

export function FloatingPill() {
  const [rawState, setRawState] = useState<VoiceWaveHudState>("idle");
  const [displayState, setDisplayState] = useState<VisualState>("idle");
  const [micLevel, setMicLevel] = useState(0);
  const [smoothedLevel, setSmoothedLevel] = useState(0);
  const [phaseTime, setPhaseTime] = useState(0);
  const [reviewItem, setReviewItem] = useState<DictionaryQueueItem | null>(null);
  const [reviewBusy, setReviewBusy] = useState(false);
  const previousRawStateRef = useRef<VoiceWaveHudState>("idle");
  // Slowly-decaying running peak for auto-gain normalization of the waveform.
  const levelPeakRef = useRef(0.08);
  // Scrolling waveform: one slot per bar, newest sample enters on the right
  // and travels left. Sampled on a fixed interval (not per-frame) so adjacent
  // bars hold meaningfully different moments of speech.
  const waveHistoryRef = useRef<number[]>(new Array(12).fill(0));
  const lastWaveSampleTsRef = useRef(0);
  const reviewRequestRef = useRef(0);
  const reviewWindowStartRef = useRef(0);
  const reviewModeActive = Boolean(reviewItem);
  const visualState: VisualState = displayState;

  // Hotkey cue sounds are played natively by the Rust core at the exact
  // state transition (see src-tauri/src/cue.rs). Webview playback lived here
  // before, but background windows get media-throttled and transition
  // detection could miss fast presses — so the cue fired inconsistently.

  const resetReview = useCallback(() => {
    reviewRequestRef.current += 1;
    setReviewItem(null);
    setReviewBusy(false);
  }, []);

  const loadReviewQueue = useCallback(async () => {
    if (!canUseTauri()) {
      return;
    }
    const requestId = reviewRequestRef.current + 1;
    reviewRequestRef.current = requestId;
    try {
      const queue = await getDictionaryQueue(12);
      if (reviewRequestRef.current !== requestId) {
        return;
      }
      const windowStart = reviewWindowStartRef.current;
      const candidate =
        windowStart > 0
          ? (queue.find((item) => item.createdAtUtcMs >= windowStart - 2500) ?? null)
          : (queue[0] ?? null);
      setReviewItem(candidate);
    } catch {
      if (reviewRequestRef.current !== requestId) {
        return;
      }
      setReviewItem(null);
    }
  }, []);

  useEffect(() => {
    if (!canUseTauri()) {
      return;
    }

    let stateUnlisten: (() => void) | null = null;
    let micUnlisten: (() => void) | null = null;

    void (async () => {
      try {
        stateUnlisten = await listenVoicewaveState((payload) => {
          setRawState(payload.state);
        });
        micUnlisten = await listenVoicewaveMicLevel((payload) => {
          setMicLevel(clamp01(payload.level ?? 0));
        });
      } catch (err) {
        // If event listening is denied (e.g. this window is missing from the
        // Tauri capability config), the waveform receives no mic levels and
        // renders flat. Fail loudly instead of silently degrading.
        console.error("voicewave-pill: event listener registration failed", err);
      }
    })();

    const snapshotTimer = window.setInterval(() => {
      void (async () => {
        try {
          const snapshot = await loadSnapshot();
          setRawState(snapshot.state);
        } catch {
          // Ignore transient snapshot poll failures in pill overlay.
        }
      })();
    }, 180);

    return () => {
      window.clearInterval(snapshotTimer);
      stateUnlisten?.();
      micUnlisten?.();
    };
  }, []);

  useEffect(() => {
    if (rawState === "inserted" || rawState === "error") {
      setDisplayState(rawState);
      const timeout = window.setTimeout(() => {
        setDisplayState("idle");
      }, 820);
      return () => {
        window.clearTimeout(timeout);
      };
    }
    setDisplayState(rawState);
    return undefined;
  }, [rawState]);

  useEffect(() => {
    const previous = previousRawStateRef.current;
    previousRawStateRef.current = rawState;

    if (rawState === "listening" || rawState === "transcribing") {
      resetReview();
      return;
    }

    if (rawState === "inserted" && previous !== "inserted") {
      reviewWindowStartRef.current = Date.now();
      void loadReviewQueue();
      const followUpTimer = window.setTimeout(() => {
        void loadReviewQueue();
      }, 420);
      const lateTimer = window.setTimeout(() => {
        void loadReviewQueue();
      }, 980);
      return () => {
        window.clearTimeout(followUpTimer);
        window.clearTimeout(lateTimer);
      };
    }
  }, [loadReviewQueue, rawState, resetReview]);

  useEffect(() => {
    if (!canUseTauri()) {
      return;
    }
    void setPillReviewMode(reviewModeActive);
  }, [reviewModeActive]);

  useEffect(
    () => () => {
      if (!canUseTauri()) {
        return;
      }
      void setPillReviewMode(false);
    },
    []
  );

  const handleApprove = useCallback(async () => {
    if (!reviewItem || reviewBusy) {
      return;
    }
    const entryId = reviewItem.entryId;
    setReviewBusy(true);
    setReviewItem(null);
    try {
      await approveDictionaryEntry(entryId);
    } catch {
      // Keep review non-blocking in the floating pill.
    } finally {
      setReviewBusy(false);
    }
  }, [reviewBusy, reviewItem]);

  const handleDismiss = useCallback(async () => {
    if (!reviewItem || reviewBusy) {
      return;
    }
    const entryId = reviewItem.entryId;
    setReviewBusy(true);
    setReviewItem(null);
    try {
      await rejectDictionaryEntry(entryId);
    } catch {
      // Keep review non-blocking in the floating pill.
    } finally {
      setReviewBusy(false);
    }
  }, [reviewBusy, reviewItem]);

  const handleLater = useCallback(() => {
    if (reviewBusy) {
      return;
    }
    resetReview();
  }, [resetReview, reviewBusy]);

  useEffect(() => {
    let frame = 0;
    let lastFrame = 0;
    let current = 0;

    const loop = (ts: number) => {
      frame = window.requestAnimationFrame(loop);
      if (ts - lastFrame < 16) {
        return;
      }
      lastFrame = ts;

      // Auto-gain: normalize the raw mic peak against a slowly-decaying
      // running maximum so loud syllables always reach the top of the range,
      // then gamma-expand (>1) to ADD contrast — quiet hovers low, speech
      // slams high. (sqrt compression made everything mid-height = flat.)
      const raw = visualState === "listening" ? micLevel : 0;
      levelPeakRef.current = Math.max(raw, levelPeakRef.current * 0.994, 0.05);
      const shaped = Math.pow(clamp01(raw / levelPeakRef.current), 1.4);
      const target =
        visualState === "listening"
          ? shaped
          : visualState === "transcribing"
            ? 0.12
            : 0.03;
      // Peak-meter dynamics: instant attack, exponential release. Any lerp on
      // the way up softens exactly the motion the user should see.
      current = target > current ? target : current * 0.86;

      // Feed the scrolling waveform at a fixed cadence (~50 ms per bar slot):
      // a syllable spans 2-4 bars, so speech reads as bumps traveling across
      // the pill instead of the whole row pumping in unison.
      if (ts - lastWaveSampleTsRef.current >= 50) {
        lastWaveSampleTsRef.current = ts;
        const history = waveHistoryRef.current;
        history.push(clamp01(current));
        history.shift();
      }

      setSmoothedLevel(clamp01(current));
      setPhaseTime(ts * 0.0075);
    };

    frame = window.requestAnimationFrame(loop);
    return () => {
      window.cancelAnimationFrame(frame);
    };
  }, [micLevel, visualState]);

  const bars = useMemo(() => {
    const history = waveHistoryRef.current;
    return history.map((value, idx) => {
      // Light neighbor blending keeps the traveling profile organic instead
      // of stair-stepped, without erasing per-bar differences.
      const prev = history[idx - 1] ?? value;
      const next = history[idx + 1] ?? value;
      const blended = 0.25 * prev + 0.5 * value + 0.25 * next;
      // Scale is capped at 1.0: a bar can fill its own height but never
      // escape the pill capsule.
      return {
        id: idx,
        scale: Math.min(1, 0.1 + blended * 0.92)
      };
    });
  }, [phaseTime, smoothedLevel]);

  return (
    <div className={`vw-pill-shell vw-pill-state-${visualState}${reviewModeActive ? " vw-pill-mode-review" : ""}`}>
      <div
        className={`vw-pill-surface${reviewModeActive ? " vw-pill-surface-review" : ""}`}
        onDoubleClick={() => {
          void showMainWindow();
        }}
        {...(reviewModeActive ? {} : { "data-tauri-drag-region": "" })}
      >
        <div className="vw-pill-glow" />

        <div className="vw-pill-core">
          <div className="vw-pill-wave">
            {bars.map((bar) => (
              <span
                key={bar.id}
                className="vw-pill-bar"
                style={{ transform: `scaleY(${bar.scale.toFixed(3)})` }}
              />
            ))}
          </div>
          <div className="vw-pill-spinner" />
        </div>

        <div className="vw-pill-review-panel" aria-hidden={!reviewModeActive}>
          <div className="vw-pill-review-head" data-tauri-drag-region>
            <p className="vw-pill-review-kicker">Dictionary Suggestion</p>
            <button
              type="button"
              className="vw-pill-review-open"
              onClick={() => {
                void showMainWindow();
              }}
            >
              Open
            </button>
          </div>
          <div className="vw-pill-review-row">
            <p className="vw-pill-review-term">Add "{reviewItem?.term ?? ""}"?</p>
            <div className="vw-pill-review-actions">
              <button
                type="button"
                className="vw-pill-action vw-pill-action-approve"
                onClick={() => void handleApprove()}
                disabled={reviewBusy}
              >
                Approve
              </button>
              <button
                type="button"
                className="vw-pill-action vw-pill-action-dismiss"
                onClick={() => void handleDismiss()}
                disabled={reviewBusy}
              >
                Dismiss
              </button>
              <button
                type="button"
                className="vw-pill-action vw-pill-action-later"
                onClick={handleLater}
                disabled={reviewBusy}
              >
                Later
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
