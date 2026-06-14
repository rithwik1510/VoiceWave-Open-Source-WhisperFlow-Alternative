import { useEffect, useState } from "react";
import { Download, X } from "lucide-react";

import {
  type AvailableUpdate,
  type DownloadProgress,
  checkForUpdate,
  installPendingUpdate
} from "../lib/updater";

type Phase = "idle" | "available" | "installing" | "error";

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

  useEffect(() => {
    let active = true;
    void (async () => {
      try {
        const found = await checkForUpdate();
        if (active && found) {
          setUpdate(found);
          setPhase("available");
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
              <h3 className="vw-section-heading text-lg font-semibold text-[#09090B]">
                Update available
              </h3>
              <p className="mt-0.5 text-sm text-[#71717A]">
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
            <div className="mt-3 max-h-40 overflow-auto rounded-2xl border border-[#E4E4E7] bg-[#FAFAFA] px-4 py-3">
              <p className="text-xs font-semibold text-[#09090B]">What's new</p>
              <pre className="mt-1 whitespace-pre-wrap font-sans text-xs text-[#52525B]">
                {update.notes}
              </pre>
            </div>
          )}

          {phase === "error" && errorMessage && (
            <div className="mt-3 rounded-2xl border border-[#f3c2c2] bg-[#fff1f1] px-4 py-3 text-sm text-[#a94444]">
              {errorMessage}
            </div>
          )}

          {installing && (
            <div className="mt-4">
              <div className="h-2 w-full overflow-hidden rounded-full bg-[#E4E4E7]">
                <div
                  className="h-full rounded-full bg-[#09090B] transition-all"
                  style={{ width: `${percent ?? 8}%` }}
                />
              </div>
              <p className="mt-2 text-xs text-[#71717A]">
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
