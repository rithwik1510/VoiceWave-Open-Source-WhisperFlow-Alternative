import { useCallback, useEffect, useRef, useState } from "react";
import { formatHotkey, hotkeyKeys } from "../lib/hotkeys";
import { listenVoicewaveHotkeyCapture } from "../lib/tauri";

type Modifier = "Ctrl" | "Alt" | "Shift" | "Windows";

// Stored order, matching the backend's canonical form.
const MODIFIER_ORDER: Modifier[] = ["Ctrl", "Alt", "Shift", "Windows"];

const MODIFIER_CODES: Record<string, Modifier> = {
  ControlLeft: "Ctrl",
  ControlRight: "Ctrl",
  AltLeft: "Alt",
  AltRight: "Alt",
  ShiftLeft: "Shift",
  ShiftRight: "Shift",
  MetaLeft: "Windows",
  MetaRight: "Windows",
  OSLeft: "Windows",
  OSRight: "Windows"
};

// Same set the backend accepts (NAMED_MAIN_KEYS in hotkey/mod.rs).
const NAMED_MAIN_KEYS = new Set(["Space", "Insert", "Home", "End", "PageUp", "PageDown", "Pause", "ScrollLock"]);

const UNSUPPORTED_KEY = "That key can't be part of a shortcut. Try a letter, a number, an F key, or Space.";

// The backend lease is 30 s; renew well inside it while recording.
const CAPTURE_RENEW_MS = 10_000;

function isModifier(token: string): token is Modifier {
  return (MODIFIER_ORDER as string[]).includes(token);
}

/** Canonical token (same as the hook reports) from `KeyboardEvent.code`. */
function tokenFromCode(code: string): string | null {
  if (code === "Escape") {
    return code;
  }
  const modifier = MODIFIER_CODES[code];
  if (modifier) {
    return modifier;
  }
  const letterOrDigit = /^(?:Key([A-Z])|Digit([0-9]))$/.exec(code);
  if (letterOrDigit) {
    return letterOrDigit[1] ?? letterOrDigit[2];
  }
  if (/^F(?:[1-9]|1[0-9]|2[0-4])$/.test(code) || NAMED_MAIN_KEYS.has(code)) {
    return code;
  }
  return null;
}

interface Capture {
  modifiers: Set<Modifier>;
  main: string | null;
}

function captureToCombo({ modifiers, main }: Capture): string {
  const parts: string[] = MODIFIER_ORDER.filter((modifier) => modifiers.has(modifier));
  if (main) {
    parts.push(main);
  }
  return parts.join("+");
}

interface HotkeyRecorderProps {
  /** Current hold-to-talk combo in stored form, e.g. "Ctrl+Windows". */
  value: string;
  defaultValue: string;
  /** Why the global keyboard hook is not running, if it is not. */
  registrationError?: string | null;
  /** Persists a combo; rejects with a plain-language reason. */
  onSave: (combo: string) => Promise<void>;
  /** Pauses the global hotkey so recording cannot start a dictation. */
  onCaptureChange: (active: boolean) => void;
}

/**
 * Settings row that records a new hold-to-talk shortcut. The combo is what is
 * held when the first key is let go, so modifier-only chords like Ctrl + Win
 * work. Esc cancels; a rejected combo keeps recording with the reason shown.
 *
 * Keys come from the global keyboard hook (it sees Win, and swallows keys so
 * Start and shell shortcuts don't fire). DOM key events are the fallback for
 * when the hook isn't running; while it is, the webview never sees the keys,
 * so the two sources never double up.
 */
export function HotkeyRecorder({ value, defaultValue, registrationError, onSave, onCaptureChange }: HotkeyRecorderProps) {
  const [recording, setRecording] = useState(false);
  const [preview, setPreview] = useState<string[]>([]);
  const [message, setMessage] = useState<string | null>(null);
  const captureRef = useRef<Capture>({ modifiers: new Set(), main: null });
  // Physically held keys: id -> token.
  const heldRef = useRef(new Map<string, string>());
  // A key went down since the last save, so the next release saves.
  const armedRef = useRef(false);
  const savingRef = useRef(false);

  const resetCapture = useCallback(() => {
    captureRef.current = { modifiers: new Set(), main: null };
    heldRef.current = new Map();
    armedRef.current = false;
    setPreview([]);
  }, []);

  const stop = useCallback(() => {
    setRecording(false);
    resetCapture();
  }, [resetCapture]);

  const start = () => {
    resetCapture();
    setMessage(null);
    setRecording(true);
  };

  // The hook stays in capture mode exactly while recording, including when
  // Settings closes mid-recording (unmount runs the cleanup).
  useEffect(() => {
    if (!recording) {
      return;
    }
    onCaptureChange(true);
    const renew = window.setInterval(() => onCaptureChange(true), CAPTURE_RENEW_MS);
    return () => {
      window.clearInterval(renew);
      onCaptureChange(false);
    };
  }, [recording, onCaptureChange]);

  const save = useCallback(
    async (combo: string) => {
      savingRef.current = true;
      try {
        await onSave(combo);
        setMessage(null);
        return true;
      } catch (err) {
        // Tauri rejects with the command's error string, not an Error.
        setMessage(err instanceof Error ? err.message : String(err));
        return false;
      } finally {
        savingRef.current = false;
      }
    },
    [onSave]
  );

  useEffect(() => {
    if (!recording) {
      return;
    }

    // `id` tells physical keys apart for the held set (DOM code, or the token
    // for hook events).
    const handleKey = (token: string | null, pressed: boolean, id: string) => {
      if (pressed) {
        if (savingRef.current) {
          return;
        }
        if (token === "Escape") {
          stop();
          return;
        }
        if (!token) {
          setMessage(UNSUPPORTED_KEY);
          return;
        }
        heldRef.current.set(id, token);
        armedRef.current = true;
        if (isModifier(token)) {
          captureRef.current.modifiers.add(token);
        } else {
          captureRef.current.main = token;
        }
        setMessage(null);
        setPreview(hotkeyKeys(captureToCombo(captureRef.current)));
        return;
      }
      if (!heldRef.current.has(id)) {
        return;
      }
      // Releases are tracked even mid-save, so after a rejection the capture
      // still holds exactly the keys that are down and the next combo builds
      // on them.
      const combo = captureToCombo(captureRef.current);
      heldRef.current.delete(id);
      const held = [...heldRef.current.values()];
      const capture = captureRef.current;
      capture.modifiers = new Set(held.filter(isModifier));
      if (capture.main && !held.includes(capture.main)) {
        capture.main = null;
      }
      setPreview(hotkeyKeys(captureToCombo(capture)));
      if (!armedRef.current) {
        return;
      }
      armedRef.current = false;
      void save(combo).then((saved) => {
        if (saved) {
          stop();
        }
      });
    };

    // Capture phase on window, and stop propagation, so the fallback path
    // sees keys before (and instead of) app shortcuts like Esc-closes-Settings.
    const onKeyDown = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();
      if (!event.repeat) {
        handleKey(tokenFromCode(event.code), true, event.code);
      }
    };
    const onKeyUp = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();
      handleKey(tokenFromCode(event.code), false, event.code);
    };

    let disposed = false;
    let unlistenHook: (() => void) | null = null;
    void listenVoicewaveHotkeyCapture(({ key, pressed }) => handleKey(key, pressed, key ?? "")).then((unlisten) => {
      if (disposed) {
        unlisten();
      } else {
        unlistenHook = unlisten;
      }
    });

    // Clicking away mid-recording cancels cleanly (the hook also stops
    // capturing as soon as VoiceWave is no longer in front).
    const onBlur = () => stop();

    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("keyup", onKeyUp, true);
    window.addEventListener("blur", onBlur);
    return () => {
      disposed = true;
      unlistenHook?.();
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("keyup", onKeyUp, true);
      window.removeEventListener("blur", onBlur);
    };
  }, [recording, save, stop]);

  const keys = recording ? preview : hotkeyKeys(value);
  const isDefault = value === defaultValue;

  return (
    <div className="vw-set-row">
      <div>
        <p className="vw-set-title">Dictation shortcut</p>
        <p className="vw-set-desc" aria-live="polite">
          {recording
            ? "Press the keys you want, then let go. Esc cancels."
            : "Hold it to talk. Double-tap it for hands-free."}
        </p>
        {message && <p className="mt-1 text-xs text-status-danger-text">{message}</p>}
        {!recording && registrationError && (
          <p className="mt-1 text-xs text-status-warn-text">{registrationError}</p>
        )}
        {!recording && !isDefault && (
          <button
            type="button"
            className="mt-1 text-xs font-semibold text-accent-text hover:underline"
            onClick={() => {
              void save(defaultValue);
            }}
          >
            Reset to default ({formatHotkey(defaultValue)})
          </button>
        )}
      </div>
      <div className="flex shrink-0 items-center gap-2">
        <div className={`vw-hotkey-field ${recording ? "vw-hotkey-field-recording" : ""}`}>
          {keys.length > 0 ? (
            keys.map((key) => (
              <kbd key={key} className="vw-keycap">
                {key}
              </kbd>
            ))
          ) : (
            <span className="text-xs text-hint">Press keys…</span>
          )}
        </div>
        <button type="button" className="vw-btn-secondary vw-btn-sm" onClick={recording ? stop : start}>
          {recording ? "Cancel" : "Change"}
        </button>
      </div>
    </div>
  );
}
