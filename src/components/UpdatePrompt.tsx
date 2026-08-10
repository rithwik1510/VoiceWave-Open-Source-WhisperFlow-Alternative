import { useEffect, useRef, useState } from "react";
import { Download, X } from "lucide-react";

import {
  type AvailableUpdate,
  type DownloadProgress,
  checkForUpdate,
  installPendingUpdate
} from "../lib/updater";
import type { PillNoticePayload } from "../types/voicewave";

type Phase = "idle" | "available" | "installing" | "error";

/**
 * Announce a ready-to-install update on the floating pill (a separate
 * always-on-top window) so it surfaces Dynamic-Island style. Best-effort: a
 * failed broadcast must never interrupt the in-app update dialog.
 */
async function announceUpdateOnPill(version: string): Promise<void> {
  try {
    const { emit } = await import("@tauri-apps/api/event");
    const notice: PillNoticePayload = {
      id: Date.now(),
      severity: "info",
      title: "Update ready",
      detail: `VoiceWave ${version} — restart to install it.`,
      durationMs: 6000,
      transcript: null,
      action: null
    };
    await emit("voicewave://pill-notice", notice);
  } catch (err) {
    console.warn("VoiceWave pill update notice failed:", err);
  }
}

function formatBytes(bytes: number): string {
  if (bytes >= 1024 * 1024) {
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }
  if (bytes >= 1024) {
    return `${(bytes / 1024).toFixed(0)} KB`;
  }
  return `${bytes} B`;
}

/**
 * Auto-update prompt. On launch it asks the GitHub release endpoint whether a
 * newer signed build exists; if so it shows a one-click "Install & Restart"
 * dialog. Renders nothing when up to date, when dismissed, or outside the
 * Tauri desktop runtime (the check simply returns null in the browser/tests).
 */
export function UpdatePrompt() {
  const [phase, setPhase] = useState<Phase>("idle");
  const [update, setUpdate] = useState<AvailableUpdate | null>(null);
  const [progress, setProgress] = useState<DownloadProgress | null>(null);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [dismissed, setDismissed] = useState(false);
  // Remembers the version we've already announced on the pill so a re-render or
  // repeated check never fires a duplicate Dynamic-Island notice.
  const announcedVersionRef = useRef<string | null>(null);

  useEffect(() => {
    let active = true;
    void (async () => {
      try {
        const found = await checkForUpdate();
        if (active && found) {
          setUpdate(found);
          setPhase("available");
          // `found` is only truthy inside the Tauri runtime (checkForUpdate
          // returns null in the browser/tests), so the pill window exists here.
          if (announcedVersionRef.current !== found.version) {
            announcedVersionRef.current = found.version;
            void announceUpdateOnPill(found.version);
          }
        }
      } catch (err) {
        // A failed check (offline, rate-limited, etc.) should never interrupt
        // the user — just stay silent and try again next launch.
        console.warn("VoiceWave update check failed:", err);
      }
    })();
    return () => {
      active = false;
    };
  }, []);

  const handleInstall = async () => {
    setPhase("installing");
    setErrorMessage(null);
    try {
      await installPendingUpdate((next) => setProgress(next));
      // On success the app relaunches, so we never fall through here.
    } catch (err) {
      setErrorMessage(
        err instanceof Error && err.message
          ? err.message
          : "The update could not be installed. Please try again later."
      );
      setPhase("error");
    }
  };

  if (dismissed || !update || phase === "idle") {
    return null;
  }

  const installing = phase === "installing";
  const percent =
    progress && progress.phase !== "started" && "downloaded" in progress && progress.contentLength
      ? Math.min(100, Math.round((progress.downloaded / progress.contentLength) * 100))
      : null;

  return (
    <div className="vw-modal-backdrop" role="presentation">
      <section
        className="vw-modal-card max-w-md"
        role="dialog"
        aria-modal="true"
        aria-label="Update available"
      >
        <header className="vw-modal-header">
          <div className="flex items-center gap-2">
            <span className="vw-pro-minimal-icon">
              <Download size={15} />
            </span>
            <div>
              <h3 className="vw-section-heading text-lg font-semibold text-ink-strong">
                Update available
              </h3>
              <p className="mt-0.5 text-sm text-faint">
                VoiceWave {update.version} is ready to install.
              </p>
            </div>
          </div>
          {!installing && (
            <button
              type="button"
              className="vw-modal-close"
              onClick={() => setDismissed(true)}
              aria-label="Dismiss update"
            >
              <X size={16} />
            </button>
          )}
        </header>

        <div className="vw-modal-body">
          <div className="flex flex-wrap items-center gap-2">
            <span className="vw-chip">Current v{update.currentVersion}</span>
            <span className="vw-chip vw-chip-accent">New v{update.version}</span>
          </div>

          {update.notes && (
            <div className="mt-3 max-h-40 overflow-auto rounded-2xl border border-edge bg-inset px-4 py-3">
              <p className="text-xs font-semibold text-ink-strong">What's new</p>
              <pre className="mt-1 whitespace-pre-wrap font-sans text-xs text-quiet">
                {update.notes}
              </pre>
            </div>
          )}

          {phase === "error" && errorMessage && (
            <div className="mt-3 rounded-2xl border border-status-danger-border bg-status-danger-bg px-4 py-3 text-sm text-status-danger-text">
              {errorMessage}
            </div>
          )}

          {installing && (
            <div className="mt-4">
              <div className="h-2 w-full overflow-hidden rounded-full bg-track">
                <div
                  className="vw-progress-fill h-full rounded-full transition-all"
                  style={{ width: `${percent ?? 8}%` }}
                />
              </div>
              <p className="mt-2 text-xs text-faint">
                {progress?.phase === "finished"
                  ? "Installing and restarting…"
                  : percent !== null
                    ? `Downloading… ${percent}%`
                    : progress && "downloaded" in progress
                      ? `Downloading… ${formatBytes(progress.downloaded)}`
                      : "Starting download…"}
              </p>
            </div>
          )}

          <div className="mt-5 flex flex-wrap justify-end gap-2">
            {!installing && (
              <button type="button" className="vw-btn-secondary" onClick={() => setDismissed(true)}>
                Later
              </button>
            )}
            <button
              type="button"
              className="vw-btn-primary vw-action-button"
              onClick={() => void handleInstall()}
              disabled={installing}
            >
              {installing ? "Installing…" : phase === "error" ? "Retry" : "Install & Restart"}
            </button>
          </div>
        </div>
      </section>
    </div>
  );
}
