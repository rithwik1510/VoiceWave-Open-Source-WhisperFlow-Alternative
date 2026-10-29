import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Lock } from "lucide-react";
import {
  addDictionaryTerm,
  approveDictionaryEntry,
  canUseTauri,
  copyTextToClipboard,
  getDictionaryQueue,
  getPillQuickControlsSnapshot,
  loadSettings,
  loadSnapshot,
  openSomethingWrong,
  rejectDictionaryEntry,
  retryLastDictation,
  listenVoicewaveMicLevel,
  listenVoicewaveDictionaryQueueChanged,
  listenVoicewavePillNotice,
  listenVoicewavePillQuickControls,
  listenVoicewaveSettingsChanged,
  listenVoicewaveState,
  setDictationProfile,
  setPillQuickControlsMode,
  setPillWindowMode,
  startDictation,
  showMainWindow
} from "../lib/tauri";
import type {
  DictationControlMode,
  DictionaryQueueItem,
  PillNoticePayload,
  PillQuickControlsInvocation,
  PolishProfile,
  PillWindowMode,
  VoiceWaveHudState
} from "../types/voicewave";
import {
  PillQuickControls,
  type QuickControlsFeedback
} from "./PillQuickControls";
import {
  derivePillDisplayMode,
  shouldAutoDismissQuickControls,
  shouldCloseQuickControlsOnKey
} from "./pillDisplayMode";

type VisualState = "idle" | "listening" | "transcribing" | "inserted" | "error";

/** Tiny persistent abbreviation of the active polish profile shown in the
 * listening capsule (plan 010). Two letters — anything longer fights the
 * waveform for space. */
const PROFILE_ABBREV: Record<PolishProfile, string> = {
  standard: "ST",
  coding: "CO",
  writing: "WR",
  casual: "CA",
  literal: "LI"
};

const PROFILE_TITLES: Record<PolishProfile, string> = {
  standard: "Standard profile",
  coding: "Coding profile",
  writing: "Writing profile",
  casual: "Casual profile",
  literal: "Literal profile"
};

const QUICK_CONTROLS_IDLE_TIMEOUT_MS = 18_000;
const QUICK_CONTROLS_CLOSE_ANIMATION_MS = 170;

interface CloseQuickControlsOptions {
  animate?: boolean;
}

function clamp01(value: number): number {
  return Math.max(0, Math.min(1, value));
}

export function FloatingPill() {
  const [rawState, setRawState] = useState<VoiceWaveHudState>("idle");
  const [controlMode, setControlMode] = useState<DictationControlMode | null>(null);
  const [displayState, setDisplayState] = useState<VisualState>("idle");
  const [smoothedLevel, setSmoothedLevel] = useState(0);
  const [phaseTime, setPhaseTime] = useState(0);
  const [reviewItem, setReviewItem] = useState<DictionaryQueueItem | null>(null);
  const [reviewBusy, setReviewBusy] = useState(false);
  // Dynamic Island notice: any runtime condition the core wants the user to
  // see (mic guard warnings, failures) expands the pill for a few seconds.
  const [notice, setNotice] = useState<PillNoticePayload | null>(null);
  // Active polish profile for the listening glyph. Settings are read at mount
  // and re-read on each listening transition (a profile can't change mid-
  // dictation, so once per dictation is enough — no polling, no new events).
  const [activeProfile, setActiveProfile] = useState<PolishProfile>("standard");
  // Actionable notices carry a one-tap button; this tracks its local
  // post-action confirmation state ("Copied ✓" / "Added ✓") shared across
  // action kinds.
  const [actionDone, setActionDone] = useState(false);
  const [quickControlsEnabled, setQuickControlsEnabled] = useState(false);
  const [quickControlsOpen, setQuickControlsOpen] = useState(false);
  const [quickControlsInvocation, setQuickControlsInvocation] =
    useState<PillQuickControlsInvocation>("mouse");
  const [pendingProfile, setPendingProfile] = useState<PolishProfile | null>(null);
  const [startingHandsFree, setStartingHandsFree] = useState(false);
  const [quickControlsFeedback, setQuickControlsFeedback] =
    useState<QuickControlsFeedback>(null);
  const [quickControlsClosing, setQuickControlsClosing] = useState(false);
  const [quickControlsSettling, setQuickControlsSettling] = useState(false);
  const noticeTimerRef = useRef<number | null>(null);
  const quickControlsTimerRef = useRef<number | null>(null);
  const quickControlsClosingRef = useRef(false);
  const previousRawStateRef = useRef<VoiceWaveHudState>("idle");
  const micLevelRef = useRef(0);
  // Slowly-decaying running peak for auto-gain normalization of the waveform.
  const levelPeakRef = useRef(0.08);
  // Scrolling waveform: one slot per bar, newest sample enters on the right
  // and travels left. Sampled on a fixed interval (not per-frame) so adjacent
  // bars hold meaningfully different moments of speech.
  const waveHistoryRef = useRef<number[]>(new Array(12).fill(0));
  const lastWaveSampleTsRef = useRef(0);
  const reviewRequestRef = useRef(0);
  const reviewWindowStartRef = useRef(0);
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

  // Notice auto-dismiss is a single-shot timer we can re-arm. Rescue notices
  // pause it on hover (clear) and resume a short grace on leave (re-schedule),
  // so the user has time to read/copy before the capsule collapses.
  const clearNoticeDismiss = useCallback(() => {
    if (noticeTimerRef.current !== null) {
      window.clearTimeout(noticeTimerRef.current);
      noticeTimerRef.current = null;
    }
  }, []);

  const scheduleNoticeDismiss = useCallback(
    (holdMs: number) => {
      clearNoticeDismiss();
      noticeTimerRef.current = window.setTimeout(() => {
        noticeTimerRef.current = null;
        setNotice(null);
      }, holdMs);
    },
    [clearNoticeDismiss]
  );

  const clearQuickControlsDismiss = useCallback(() => {
    if (quickControlsTimerRef.current !== null) {
      window.clearTimeout(quickControlsTimerRef.current);
      quickControlsTimerRef.current = null;
    }
  }, []);

  const closeQuickControls = useCallback(async (
    options: CloseQuickControlsOptions = {}
  ) => {
    if (quickControlsClosingRef.current) {
      return false;
    }

    clearQuickControlsDismiss();
    quickControlsClosingRef.current = true;
    const shouldAnimate =
      (options.animate ?? true) &&
      !window.matchMedia("(prefers-reduced-motion: reduce)").matches;

    try {
      if (shouldAnimate) {
        setQuickControlsClosing(true);
        await new Promise<void>((resolve) => {
          window.setTimeout(resolve, QUICK_CONTROLS_CLOSE_ANIMATION_MS);
        });
      }

      if (!canUseTauri()) {
        setQuickControlsOpen(false);
        return true;
      }

      await setPillQuickControlsMode(false);
      // Native has now resized the overlay. Snap the compact geometry into
      // place for one frame so it does not replay a second shrink animation.
      setQuickControlsSettling(true);
      setQuickControlsOpen(false);
      window.requestAnimationFrame(() => {
        window.requestAnimationFrame(() => setQuickControlsSettling(false));
      });
      return true;
    } catch {
      // Keep the card truthful and operable if native geometry/focus restore
      // failed; starting dictation from a half-collapsed card is unsafe.
      setQuickControlsOpen(true);
      return false;
    } finally {
      setQuickControlsClosing(false);
      quickControlsClosingRef.current = false;
    }
  }, [clearQuickControlsDismiss]);

  const scheduleQuickControlsDismiss = useCallback(() => {
    clearQuickControlsDismiss();
    quickControlsTimerRef.current = window.setTimeout(() => {
      quickControlsTimerRef.current = null;
      void closeQuickControls();
    }, QUICK_CONTROLS_IDLE_TIMEOUT_MS);
  }, [clearQuickControlsDismiss, closeQuickControls]);

  const refreshActiveProfile = useCallback(async () => {
    if (!canUseTauri()) {
      return;
    }
    try {
      const settings = await loadSettings();
      // Absent on backends older than plan 010 — glyph falls back to Standard.
      setActiveProfile(settings.polishProfile ?? "standard");
    } catch {
      // Keep the last known profile; the glyph is informational only.
    }
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

    // Cleanup can run before the awaited registrations finish (StrictMode
    // remounts, HMR); a listener that resolves after cleanup is released on
    // the spot instead of leaking a duplicate handler.
    let disposed = false;
    const unlisteners: Array<() => void> = [];
    const keep = async (pending: Promise<() => void>) => {
      const unlisten = await pending;
      if (disposed) {
        unlisten();
      } else {
        unlisteners.push(unlisten);
      }
    };

    void (async () => {
      try {
        await keep(listenVoicewaveState((payload) => {
          setRawState(payload.state);
          setControlMode((previous) =>
            payload.controlMode ?? (payload.state === "idle" ? null : previous)
          );
        }));
        await keep(listenVoicewaveMicLevel((payload) => {
          // The waveform loop reads the latest sample directly. Keeping raw
          // mic events out of React state avoids a render for every native
          // level update while listening.
          micLevelRef.current = clamp01(payload.level ?? 0);
        }));
        await keep(listenVoicewavePillNotice((payload) => {
          setNotice(payload);
          // Rescue notices (transcript present) may hold their full duration up
          // to 10s so the user can read/copy; plain notices keep the 2.5-12s
          // clamp. A Try again offer may hold 30s: it can be the only way back
          // to a long dictation.
          const maxHoldMs = payload.action?.kind === "retryDictation" ? 30000 : 12000;
          const holdMs = Math.min(Math.max(payload.durationMs || 5000, 2500), maxHoldMs);
          scheduleNoticeDismiss(holdMs);
        }));
        await keep(listenVoicewavePillQuickControls((payload) => {
          // A locally initiated animated close owns the final state change.
          // Ignoring its echoed native event prevents React from switching to
          // compact CSS before the exit transition has completed.
          if (!payload.open && quickControlsClosingRef.current) {
            return;
          }
          if (payload.invocation) {
            setQuickControlsInvocation(payload.invocation);
          }
          if (payload.open) {
            quickControlsClosingRef.current = false;
            setQuickControlsClosing(false);
            setQuickControlsSettling(false);
          }
          setQuickControlsOpen(payload.open);
          setQuickControlsFeedback(null);
          if (payload.open) {
            void refreshActiveProfile();
          }
        }));
        await keep(listenVoicewaveSettingsChanged((settings) => {
          setActiveProfile(settings.polishProfile ?? "standard");
        }));
        await keep(listenVoicewaveDictionaryQueueChanged(() => {
          void loadReviewQueue();
        }));
      } catch (err) {
        // If event listening is denied (e.g. this window is missing from the
        // Tauri capability config), the waveform receives no mic levels and
        // renders flat. Fail loudly instead of silently degrading.
        console.error("voicewave-pill: event listener registration failed", err);
      }
    })();

    // Slow reconciliation safety net, NOT the primary state source: the
    // voicewave://state event above drives updates. This low-frequency poll
    // only exists so a dropped event can't strand the pill in a stale state.
    // Kept deliberately coarse (2s) to avoid contending for the controller's
    // snapshot lock that the dictation path holds.
    const snapshotTimer = window.setInterval(() => {
      void (async () => {
        try {
          const snapshot = await loadSnapshot();
          setRawState(snapshot.state);
          setControlMode((previous) =>
            snapshot.controlMode ?? (snapshot.state === "idle" ? null : previous)
          );
        } catch {
          // Ignore transient snapshot poll failures in pill overlay.
        }
      })();
    }, 2000);

    return () => {
      window.clearInterval(snapshotTimer);
      disposed = true;
      for (const unlisten of unlisteners) {
        unlisten();
      }
      if (noticeTimerRef.current !== null) {
        window.clearTimeout(noticeTimerRef.current);
      }
      if (quickControlsTimerRef.current !== null) {
        window.clearTimeout(quickControlsTimerRef.current);
      }
    };
  }, [loadReviewQueue, refreshActiveProfile, scheduleNoticeDismiss]);

  useEffect(() => {
    if (!canUseTauri()) {
      return;
    }
    void (async () => {
      try {
        const snapshot = await getPillQuickControlsSnapshot();
        setQuickControlsEnabled(snapshot.enabled);
        setQuickControlsInvocation(snapshot.invocation ?? "mouse");
        setQuickControlsOpen(snapshot.enabled && snapshot.invocation !== null);
      } catch {
        // Missing/failed Phase 0 backend keeps the feature off by default.
        setQuickControlsEnabled(false);
        setQuickControlsOpen(false);
      }
    })();
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

  // Initial profile fetch so the glyph is correct on the very first dictation.
  useEffect(() => {
    void refreshActiveProfile();
  }, [refreshActiveProfile]);

  useEffect(() => {
    const previous = previousRawStateRef.current;
    previousRawStateRef.current = rawState;

    if (rawState === "listening" || rawState === "transcribing") {
      if (rawState === "listening" && previous !== "listening") {
        void refreshActiveProfile();
      }
      resetReview();
      return;
    }

    if (rawState === "inserted" && previous !== "inserted") {
      reviewWindowStartRef.current = Date.now();
    }
  }, [loadReviewQueue, rawState, refreshActiveProfile, resetReview]);

  // A new dictation supersedes the last one's Try again card (recovery
  // notices outrank the live status). If the new one fails, its own card
  // arrives later with a fresh dismiss timer.
  const retryNoticeShown = notice?.action?.kind === "retryDictation";
  useEffect(() => {
    if (rawState === "listening" && retryNoticeShown) {
      clearNoticeDismiss();
      setNotice(null);
    }
  }, [clearNoticeDismiss, rawState, retryNoticeShown]);

  const pillDisplayMode = derivePillDisplayMode({
    visualState,
    quickControlsEnabled,
    quickControlsOpen,
    quickControlsInvocation,
    reviewItem,
    notice
  });
  const reviewModeActive = pillDisplayMode.kind === "review";
  const noticeActive = pillDisplayMode.kind === "notice";
  const quickControlsActive = pillDisplayMode.kind === "quickControls";
  // Only actionable notices make the OS window clickable; plain notices stay
  // click-through even while visually expanded.
  const noticeInteractive = noticeActive && notice?.action != null;
  const nativeWindowMode: PillWindowMode = reviewModeActive
    ? "review"
    : noticeActive
      ? "notice"
      : "compact";

  useEffect(() => {
    if (quickControlsActive && shouldAutoDismissQuickControls(quickControlsInvocation)) {
      scheduleQuickControlsDismiss();
      return clearQuickControlsDismiss;
    }
    clearQuickControlsDismiss();
    return undefined;
  }, [
    clearQuickControlsDismiss,
    quickControlsActive,
    quickControlsInvocation,
    scheduleQuickControlsDismiss
  ]);

  useEffect(() => {
    if (!quickControlsActive || quickControlsInvocation !== "focused") {
      return;
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      if (!shouldCloseQuickControlsOnKey(quickControlsInvocation, event.key)) {
        return;
      }
      event.preventDefault();
      event.stopPropagation();
      void closeQuickControls();
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, [closeQuickControls, quickControlsActive, quickControlsInvocation]);

  useEffect(() => {
    if (quickControlsOpen && pillDisplayMode.kind !== "quickControls") {
      void closeQuickControls({ animate: false });
    }
  }, [closeQuickControls, pillDisplayMode.kind, quickControlsOpen]);

  useEffect(() => {
    // When a higher-priority state preempts an open card, let the native close
    // finish first. Issuing Notice/Review geometry concurrently with that
    // close would recreate the exact resize race this arbiter removes.
    if (!canUseTauri() || quickControlsOpen) {
      return;
    }
    void setPillWindowMode(nativeWindowMode, noticeInteractive);
  }, [nativeWindowMode, noticeInteractive, quickControlsOpen]);

  // A fresh notice id means a new message — clear any stale confirmation state.
  useEffect(() => {
    setActionDone(false);
  }, [notice?.id]);

  const handleCopyTranscript = useCallback(async () => {
    const transcript = notice?.transcript;
    if (!transcript) {
      return;
    }
    try {
      await copyTextToClipboard(transcript);
      setActionDone(true);
      // Keep the rescue capsule up a beat so the confirmation registers, then
      // collapse it.
      scheduleNoticeDismiss(1200);
    } catch {
      // Copy failed (clipboard denied): leave the button as "Copy" and keep the
      // notice open so the user can retry. Never crash the overlay.
    }
  }, [notice, scheduleNoticeDismiss]);

  const handleAddDictionaryTerm = useCallback(async () => {
    const term = notice?.action?.value;
    if (!term) {
      return;
    }
    try {
      await addDictionaryTerm(term);
      setActionDone(true);
      // Keep the capsule up a beat so "Added ✓" registers, then collapse.
      scheduleNoticeDismiss(1200);
    } catch {
      // Add failed: leave the button so the user can retry. Never crash the overlay.
    }
  }, [notice, scheduleNoticeDismiss]);

  const handleRetryDictation = useCallback(async () => {
    try {
      await retryLastDictation();
      // The pill moves to transcribing; the offer has done its job.
      clearNoticeDismiss();
      setNotice(null);
    } catch {
      // Nothing left to retry, or a dictation is running: keep the notice so
      // the user can try again in a moment. Never crash the overlay.
    }
  }, [clearNoticeDismiss]);

  // Hover pause is only wired for interactive rescue notices: holding the
  // pointer over the capsule clears the dismiss timer; leaving re-arms a short
  // 2s grace.
  const handleNoticeMouseEnter = useCallback(() => {
    clearNoticeDismiss();
  }, [clearNoticeDismiss]);

  const handleNoticeMouseLeave = useCallback(() => {
    scheduleNoticeDismiss(2000);
  }, [scheduleNoticeDismiss]);

  const handleSelectProfile = useCallback(
    async (profile: PolishProfile) => {
      if (pendingProfile || startingHandsFree) {
        return;
      }
      if (profile === activeProfile) {
        setQuickControlsFeedback({
          tone: "success",
          message: `${PROFILE_TITLES[profile]} is already active`
        });
        scheduleQuickControlsDismiss();
        return;
      }
      setPendingProfile(profile);
      setQuickControlsFeedback(null);
      try {
        const settings = await setDictationProfile(profile);
        setActiveProfile(settings.polishProfile ?? profile);
        setQuickControlsFeedback({
          tone: "success",
          message: `${PROFILE_TITLES[profile]} is ready`
        });
      } catch {
        setQuickControlsFeedback({
          tone: "error",
          message: "Couldn’t change profile — try again"
        });
      } finally {
        setPendingProfile(null);
        scheduleQuickControlsDismiss();
      }
    },
    [activeProfile, pendingProfile, scheduleQuickControlsDismiss, startingHandsFree]
  );

  const handleStartHandsFree = useCallback(async () => {
    if (pendingProfile || startingHandsFree) {
      return;
    }
    setStartingHandsFree(true);
    setQuickControlsFeedback(null);
    try {
      // Focused tray entry must restore the prior app before dictation starts.
      // The mouse path is also collapsed first for one predictable contract.
      const closed = await closeQuickControls({ animate: false });
      if (!closed) {
        setQuickControlsFeedback({
          tone: "error",
          message: "Couldn’t close safely — try again"
        });
        return;
      }
      await startDictation("microphone", "handsFree");
    } catch {
      // The card is already safely collapsed; runtime failures continue to
      // use VoiceWave's existing notice/recovery surface.
    } finally {
      setStartingHandsFree(false);
    }
  }, [closeQuickControls, pendingProfile, startingHandsFree]);

  // Open, or "Something wrong?" (`openSomethingWrong`), after the card closes.
  const handleOpenMain = useCallback(async (open: () => Promise<void>) => {
    const closed = await closeQuickControls();
    if (!closed) {
      setQuickControlsFeedback({
        tone: "error",
        message: "Couldn’t close safely — try again"
      });
      return;
    }
    try {
      await open();
    } catch {
      // Main-window failures are rare and the status pill is already restored;
      // do not strand an invisible interactive window on top of the target app.
    }
  }, [closeQuickControls]);

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
    if (visualState !== "listening") {
      // Idle, transcribing, notice, and review visuals are CSS-driven. Do not
      // keep a WebView animation-frame loop alive while the waveform is hidden.
      levelPeakRef.current = 0.08;
      waveHistoryRef.current.fill(0);
      lastWaveSampleTsRef.current = 0;
      setSmoothedLevel(0);
      setPhaseTime(0);
      return;
    }

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
      const raw = micLevelRef.current;
      levelPeakRef.current = Math.max(raw, levelPeakRef.current * 0.994, 0.05);
      const shaped = Math.pow(clamp01(raw / levelPeakRef.current), 1.4);
      const target = shaped;
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
  }, [visualState]);

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

  const handsFreeActive = controlMode === "handsFree" && visualState === "listening";
  const pillLabel = handsFreeActive
    ? "VoiceWave hands-free listening. Press the shortcut again when you're done."
    : `VoiceWave ${visualState}`;

  return (
    <div
      className={`vw-pill-shell vw-pill-state-${visualState}${handsFreeActive ? " vw-pill-control-hands-free" : ""}${reviewModeActive ? " vw-pill-mode-review" : ""}${noticeActive ? ` vw-pill-mode-notice vw-pill-notice-${notice?.severity ?? "info"}${notice?.transcript ? " vw-pill-mode-rescue" : ""}` : ""}${quickControlsActive ? ` vw-pill-mode-quick-controls vw-pill-quick-${quickControlsInvocation}${quickControlsClosing ? " vw-pill-quick-closing" : ""}` : ""}${quickControlsSettling ? " vw-pill-quick-settling" : ""}`}
    >
      <div
        className={`vw-pill-surface${reviewModeActive ? " vw-pill-surface-review" : ""}`}
        role={quickControlsActive ? undefined : reviewModeActive || noticeInteractive ? "dialog" : "status"}
        aria-label={
          quickControlsActive
            ? "VoiceWave Quick Controls"
            : reviewModeActive
              ? "VoiceWave dictionary suggestion"
              : noticeInteractive
                ? "VoiceWave action required"
                : pillLabel
        }
        title={quickControlsActive ? "VoiceWave Quick Controls" : pillLabel}
        {...(reviewModeActive || quickControlsActive
          ? {}
          : { "data-tauri-drag-region": "" })}
      >
        <div className="vw-pill-glow" />

        <div className="vw-pill-core">
          <span className="vw-pill-profile" title={PROFILE_TITLES[activeProfile]}>
            {PROFILE_ABBREV[activeProfile]}
          </span>
          <span className="vw-pill-control-indicator" aria-hidden="true">
            <Lock size={8} strokeWidth={2.5} />
          </span>
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

        {quickControlsActive ? (
          <PillQuickControls
            invocation={quickControlsInvocation}
            activeProfile={activeProfile}
            pendingProfile={pendingProfile}
            startingHandsFree={startingHandsFree}
            feedback={quickControlsFeedback}
            onSelectProfile={(profile) => void handleSelectProfile(profile)}
            onStartHandsFree={() => void handleStartHandsFree()}
            onOpenMain={() => void handleOpenMain(showMainWindow)}
            onSomethingWrong={() => void handleOpenMain(openSomethingWrong)}
            onClose={() => void closeQuickControls()}
            onActivity={scheduleQuickControlsDismiss}
          />
        ) : null}

        <div
          className="vw-pill-notice-panel"
          aria-hidden={!noticeActive}
          role="status"
          {...(noticeInteractive
            ? { onMouseEnter: handleNoticeMouseEnter, onMouseLeave: handleNoticeMouseLeave }
            : {})}
        >
          <div className="vw-pill-notice-top">
            <span className="vw-pill-notice-dot" />
            <div className="vw-pill-notice-text">
              <p className="vw-pill-notice-title">{notice?.title ?? ""}</p>
              {notice?.detail ? <p className="vw-pill-notice-detail">{notice.detail}</p> : null}
            </div>
            {notice?.action?.kind === "copyTranscript" ? (
              <button
                type="button"
                className={`vw-pill-action vw-pill-action-copy${actionDone ? " is-copied" : ""}`}
                onClick={() => void handleCopyTranscript()}
              >
                {actionDone ? "Copied ✓" : "Copy"}
              </button>
            ) : notice?.action?.kind === "addDictionaryTerm" ? (
              <button
                type="button"
                className={`vw-pill-action vw-pill-action-add${actionDone ? " is-done" : ""}`}
                onClick={() => void handleAddDictionaryTerm()}
              >
                {actionDone ? "Added ✓" : notice.action.label}
              </button>
            ) : notice?.action?.kind === "retryDictation" ? (
              <button
                type="button"
                className="vw-pill-action vw-pill-action-retry"
                onClick={() => void handleRetryDictation()}
              >
                {notice.action.label}
              </button>
            ) : null}
          </div>
          {notice?.transcript ? (
            <p className="vw-pill-notice-transcript">{notice.transcript}</p>
          ) : null}
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
