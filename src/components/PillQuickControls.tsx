import { useEffect, useRef } from "react";
import { ExternalLink, MessageSquareWarning, Mic, X } from "lucide-react";
import type { PillQuickControlsInvocation, PolishProfile } from "../types/voicewave";

export type QuickControlsFeedback =
  | { tone: "success" | "error"; message: string }
  | null;

interface PillQuickControlsProps {
  invocation: PillQuickControlsInvocation;
  activeProfile: PolishProfile;
  pendingProfile: PolishProfile | null;
  startingHandsFree: boolean;
  feedback: QuickControlsFeedback;
  onSelectProfile: (profile: PolishProfile) => void;
  onStartHandsFree: () => void;
  onOpenMain: () => void;
  /** Opens the main window on Help's "Something wrong?". */
  onSomethingWrong: () => void;
  onClose: () => void;
  onActivity: () => void;
}

const PROFILES: ReadonlyArray<{ id: PolishProfile; label: string }> = [
  { id: "standard", label: "Standard" },
  { id: "writing", label: "Writing" },
  { id: "coding", label: "Coding" },
  { id: "literal", label: "Literal" }
];

const PROFILE_LABELS: Record<PolishProfile, string> = {
  standard: "Standard",
  writing: "Writing",
  coding: "Coding",
  casual: "Casual",
  literal: "Literal"
};

export function PillQuickControls({
  invocation,
  activeProfile,
  pendingProfile,
  startingHandsFree,
  feedback,
  onSelectProfile,
  onStartHandsFree,
  onOpenMain,
  onSomethingWrong,
  onClose,
  onActivity
}: PillQuickControlsProps) {
  const closeRef = useRef<HTMLButtonElement>(null);
  const profileRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const busy = pendingProfile !== null || startingHandsFree;
  const caption = feedback?.message ??
    (activeProfile === "casual"
      ? "Casual is active — choose a profile to switch"
      : `${PROFILE_LABELS[activeProfile]} formats your next dictation`);

  useEffect(() => {
    if (invocation === "focused") {
      closeRef.current?.focus();
    }
  }, [invocation]);

  return (
    <div
      className="vw-pill-quick-panel"
      role="dialog"
      aria-modal="false"
      aria-labelledby="vw-pill-quick-title"
      onPointerEnter={onActivity}
      onFocusCapture={onActivity}
    >
      <div className="vw-pill-quick-heading">
        <span className="vw-pill-quick-mark" aria-hidden="true" />
        <div className="vw-pill-quick-copy">
          <p id="vw-pill-quick-title" className="vw-pill-quick-title">Quick Controls</p>
          <p
            className={`vw-pill-quick-caption${feedback ? ` is-${feedback.tone}` : ""}`}
            aria-live="polite"
          >
            {caption}
          </p>
        </div>
        <button
          type="button"
          className="vw-pill-quick-open"
          onClick={onOpenMain}
          disabled={busy}
        >
          Open
          <ExternalLink size={11} aria-hidden="true" />
        </button>
        <button
          type="button"
          className="vw-pill-quick-report"
          aria-label="Something wrong?"
          title="Something wrong?"
          onClick={onSomethingWrong}
          disabled={busy}
        >
          <MessageSquareWarning size={14} aria-hidden="true" />
        </button>
        <button
          ref={closeRef}
          type="button"
          className="vw-pill-quick-close"
          aria-label="Close Quick Controls"
          onClick={onClose}
        >
          <X size={14} aria-hidden="true" />
        </button>
      </div>

      <div className="vw-pill-quick-profiles" role="radiogroup" aria-label="Dictation profile">
        {PROFILES.map((profile, index) => {
          const selected = activeProfile === profile.id;
          const pending = pendingProfile === profile.id;
          return (
            <button
              ref={(node) => {
                profileRefs.current[index] = node;
              }}
              key={profile.id}
              type="button"
              className={`vw-pill-quick-profile${selected ? " is-selected" : ""}${pending ? " is-pending" : ""}`}
              role="radio"
              aria-checked={selected}
              aria-busy={pending}
              tabIndex={selected || (activeProfile === "casual" && index === 0) ? 0 : -1}
              disabled={busy}
              onClick={() => onSelectProfile(profile.id)}
              onKeyDown={(event) => {
                if (!["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(event.key)) {
                  return;
                }
                event.preventDefault();
                const direction = event.key === "ArrowLeft" || event.key === "ArrowUp" ? -1 : 1;
                const nextIndex = (index + direction + PROFILES.length) % PROFILES.length;
                profileRefs.current[nextIndex]?.focus();
                onSelectProfile(PROFILES[nextIndex].id);
              }}
            >
              <span>{profile.label}</span>
            </button>
          );
        })}
      </div>

      <button
        type="button"
        className="vw-pill-quick-hands-free"
        onClick={onStartHandsFree}
        disabled={busy}
      >
        <Mic size={14} aria-hidden="true" />
        <span>{startingHandsFree ? "Starting…" : "Start hands-free"}</span>
      </button>
    </div>
  );
}
