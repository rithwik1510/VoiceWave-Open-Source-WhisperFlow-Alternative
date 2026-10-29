import { useState } from "react";
import { track } from "../lib/tauri";
import type { FeedbackKind, FeedbackSurface } from "../types/voicewave";
import { SaveSupportFileButton, SupportFileStatus, type SupportFile } from "./SupportFile";

const REASONS: ReadonlyArray<{ kind: FeedbackKind; label: string }> = [
  { kind: "wrong_words", label: "It heard the wrong words" },
  { kind: "missed_words", label: "It missed words" },
  { kind: "too_slow", label: "It's too slow" },
  { kind: "text_did_not_appear", label: "The text didn't appear" },
  { kind: "wrong_place", label: "The text went to the wrong place" },
  { kind: "froze_or_crashed", label: "It froze or crashed" },
  { kind: "hotkey", label: "The shortcut didn't work" },
  { kind: "other", label: "Something else" }
];

/** "Something wrong?" (plan 024 section 8.1), the first step of Help → Report
 * a problem: pick a reason, then tell us more on GitHub or save a support
 * file. Only the reason and `surface` go to analytics, and only when sharing
 * is on (the backend's consent gate); never any text. */
export function SomethingWrong({
  surface,
  sharing,
  supportFile,
  onTellUsMore
}: {
  surface: FeedbackSurface;
  /** Anonymous reports are on and answered for this version. */
  sharing: boolean;
  supportFile: SupportFile;
  onTellUsMore: (reason: FeedbackKind) => void;
}) {
  const [picked, setPicked] = useState<FeedbackKind | null>(null);

  if (!picked) {
    return (
      <>
        <p className="mt-1 text-sm text-faint">Something wrong? Pick what happened.</p>
        <div className="mt-3 flex flex-wrap gap-2" role="group" aria-label="What went wrong">
          {REASONS.map((reason) => (
            <button
              key={reason.kind}
              type="button"
              className="vw-btn-secondary vw-btn-sm"
              onClick={() => {
                setPicked(reason.kind);
                void track({ event: "feedback_reported", kind: reason.kind, surface });
              }}
            >
              {reason.label}
            </button>
          ))}
        </div>
      </>
    );
  }

  return (
    <>
      <p className="mt-1 text-sm text-faint">
        Thanks for telling us. {sharing ? "We'll get the reason you picked, and nothing else." : "Nothing was sent."}{" "}
        To help us fix it, tell us more on GitHub or save a support file to attach.
      </p>
      <div className="mt-3 flex flex-wrap gap-2">
        <button type="button" className="vw-btn-primary vw-btn-sm" onClick={() => onTellUsMore(picked)}>
          Tell us more on GitHub
        </button>
        <SaveSupportFileButton file={supportFile} />
      </div>
      <SupportFileStatus state={supportFile.state} />
      <p className="mt-2 text-xs text-hint">
        GitHub opens a pre-filled issue with the diagnostics report: app and Windows versions, speech engine,
        microphone name and recent warnings. Opening the form sends these to GitHub to fill it in; submitting it
        publishes the issue.
      </p>
    </>
  );
}
