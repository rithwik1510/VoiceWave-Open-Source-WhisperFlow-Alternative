import { useEffect, useState } from "react";

import {
  type AvailableUpdate,
  type DownloadProgress,
  checkForUpdate,
  installPendingUpdate
} from "../lib/updater";
import { canUseTauri } from "../lib/tauri";

type CheckPhase = "idle" | "checking" | "up-to-date" | "available" | "installing" | "error";

/**
 * The Settings → Updates pane: shows the installed version and lets the user
 * check for (and install) updates on demand, instead of only relying on the
 * silent launch-time check. Reuses the same updater plumbing as UpdatePrompt,
 * so an install started here follows the identical signed-update path.
 */
export function UpdateSection() {
  const [appVersion, setAppVersion] = useState<string | null>(null);
  const [phase, setPhase] = useState<CheckPhase>("idle");
  const [update, setUpdate] = useState<AvailableUpdate | null>(null);
  const [progress, setProgress] = useState<DownloadProgress | null>(null);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const desktop = canUseTauri();

  useEffect(() => {
    let active = true;
    void (async () => {
      try {
        const { getVersion } = await import("@tauri-apps/api/app");
        const version = await getVersion();
        if (active) {
          setAppVersion(version);
        }
      } catch {
        // Browser / test runtime: no Tauri API. Leave the version unknown.
      }
    })();
    return () => {
      active = false;
    };
  }, []);

  const handleCheck = async () => {
    setPhase("checking");
    setErrorMessage(null);
    try {
      const found = await checkForUpdate();
      if (found) {
        setUpdate(found);
        setPhase("available");
      } else {
        setUpdate(null);
        setPhase("up-to-date");
      }
    } catch (err) {
      setErrorMessage(
        err instanceof Error && err.message
          ? err.message
          : "Could not reach the update server. Check your connection and try again."
      );
      setPhase("error");
    }
  };

  const handleInstall = async () => {
    setPhase("installing");
    setErrorMessage(null);
    try {
      await installPendingUpdate((next) => setProgress(next));
      // On success the app relaunches into the new version.
    } catch (err) {
      setErrorMessage(
        err instanceof Error && err.message
          ? err.message
          : "The update could not be installed. Please try again later."
      );
      setPhase("error");
    }
  };

  const installing = phase === "installing";
  const percent =
    progress && progress.phase !== "started" && "downloaded" in progress && progress.contentLength
      ? Math.min(100, Math.round((progress.downloaded / progress.contentLength) * 100))
      : null;

  return (
    <div>
      <div className="vw-set-row">
        <div>
          <p className="vw-set-title">Current version</p>
          <p className="vw-set-desc">VoiceWave Local Core, installed on this machine.</p>
        </div>
        <span className="vw-chip">{appVersion ? `v${appVersion}` : "dev build"}</span>
      </div>

      <div className="vw-set-row">
        <div>
          <p className="vw-set-title">Check for updates</p>
          <p className="vw-set-desc">
            {desktop
              ? "Fetches the latest signed release. Updates also check automatically at launch."
              : "Update checks run in the desktop app."}
          </p>
        </div>
        <button
          type="button"
          className="vw-btn-secondary vw-btn-sm"
          onClick={() => void handleCheck()}
          disabled={!desktop || phase === "checking" || installing}
        >
          {phase === "checking" ? "Checking…" : "Check now"}
        </button>
      </div>

      {phase === "up-to-date" && (
        <div className="vw-set-row">
          <div className="flex items-center gap-2">
            <span aria-hidden className="h-2 w-2 rounded-full bg-status-success" />
            <p className="text-sm text-ink-strong">You're up to date.</p>
          </div>
        </div>
      )}

      {phase === "error" && errorMessage && (
        <div className="mt-3 rounded-2xl border border-status-danger-border bg-status-danger-bg px-4 py-3 text-sm text-status-danger-text">
          {errorMessage}
        </div>
      )}

      {(phase === "available" || installing || (phase === "error" && update)) && update && (
        <div className="mt-3 rounded-2xl border border-edge bg-inset px-4 py-3">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div className="flex flex-wrap items-center gap-2">
              <span className="vw-chip vw-chip-accent">New v{update.version}</span>
              <p className="text-sm text-ink-strong">VoiceWave {update.version} is ready to install.</p>
            </div>
            <button
              type="button"
              className="vw-btn-primary vw-btn-sm"
              onClick={() => void handleInstall()}
              disabled={installing}
            >
              {installing ? "Installing…" : phase === "error" ? "Retry install" : "Install & Restart"}
            </button>
          </div>
          {update.notes && !installing && (
            <pre className="mt-2 max-h-32 overflow-auto whitespace-pre-wrap font-sans text-xs text-quiet">
              {update.notes}
            </pre>
          )}
          {installing && (
            <div className="mt-3">
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
                    : "Starting download…"}
              </p>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
