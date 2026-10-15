import { useEffect, useState } from "react";

import {
  type GpuRuntimeStatus,
  cancelGpuRuntimeInstall,
  getGpuRuntimeStatus,
  installGpuRuntime,
  listenGpuRuntimeProgress,
  verifyGpuRuntime
} from "../lib/gpuRuntime";
import { canUseTauri } from "../lib/tauri";

interface GpuAccelerationProps {
  modelId: string;
}

function formatBytes(bytes: number): string {
  const units = ["B", "KB", "MB", "GB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value >= 10 || unit === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[unit]}`;
}

function errorMessage(error: unknown): string {
  return error instanceof Error && error.message
    ? error.message
    : "NVIDIA acceleration could not be prepared. Try again.";
}

export function GpuAcceleration({ modelId }: GpuAccelerationProps) {
  const desktop = canUseTauri();
  const [status, setStatus] = useState<GpuRuntimeStatus | null>(null);
  const [loading, setLoading] = useState(desktop);
  const [actionPending, setActionPending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!desktop) {
      return;
    }

    let active = true;
    let unlisten: (() => void) | undefined;
    void listenGpuRuntimeProgress((next) => {
      if (active) {
        setStatus(next);
        setError(null);
      }
    }).then((stop) => {
      if (active) {
        unlisten = stop;
      } else {
        stop();
      }
    });
    void getGpuRuntimeStatus()
      .then((next) => {
        if (active) {
          setStatus(next);
        }
      })
      .catch((reason: unknown) => {
        if (active) {
          setError(errorMessage(reason));
        }
      })
      .finally(() => {
        if (active) {
          setLoading(false);
        }
      });

    return () => {
      active = false;
      unlisten?.();
    };
  }, [desktop]);

  useEffect(() => {
    const busy = status?.state === "downloading" || status?.state === "installing" || status?.state === "verifying";
    if (!desktop || !busy) {
      return;
    }
    const timer = window.setInterval(() => {
      void getGpuRuntimeStatus().then(setStatus).catch(() => {
        // The command result and progress event still own visible errors.
      });
    }, 750);
    return () => window.clearInterval(timer);
  }, [desktop, status?.state]);

  const handleInstall = async () => {
    setActionPending(true);
    setError(null);
    try {
      const installed = await installGpuRuntime();
      setStatus(installed);
      if (installed.state === "ready" && !installed.restartRequired && !installed.cudaVerified) {
        setStatus(await verifyGpuRuntime(modelId));
      }
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setActionPending(false);
    }
  };

  const handleCancel = async () => {
    setActionPending(true);
    setError(null);
    try {
      setStatus(await cancelGpuRuntimeInstall());
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setActionPending(false);
    }
  };

  const handleVerify = async () => {
    setActionPending(true);
    setError(null);
    try {
      setStatus(await verifyGpuRuntime(modelId));
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setActionPending(false);
    }
  };

  const busy = status?.state === "downloading" || status?.state === "installing" || status?.state === "verifying";
  const downloaded = status?.downloadedBytes;
  const total = status?.totalBytes;
  const percent = status?.percent ?? (downloaded !== null && downloaded !== undefined && total ? (downloaded / total) * 100 : null);
  const ready = status?.state === "ready";
  const failed = status?.state === "failed" || status?.state === "cancelled";

  let summary = "Use your NVIDIA GPU for faster local transcription.";
  if (!desktop) summary = "NVIDIA acceleration is available in the desktop app.";
  else if (loading) summary = "Checking this PC…";
  else if (status?.message) summary = status.message;
  else if (ready && status.cudaVerified) summary = "NVIDIA acceleration is on and verified.";

  return (
    <section className="vw-panel vw-panel-soft mt-2" aria-labelledby="gpu-acceleration-title">
      <div className="flex flex-wrap items-center justify-between gap-4">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <h3 id="gpu-acceleration-title" className="text-sm font-semibold text-ink-strong">
              NVIDIA acceleration
            </h3>
            {ready && status.cudaVerified && <span className="vw-chip vw-chip-accent">Verified</span>}
            {ready && status.restartRequired && <span className="vw-chip">Restart needed</span>}
          </div>
          <p className="mt-1 text-xs text-faint" aria-live="polite">{summary}</p>
        </div>

        <div className="flex items-center gap-2">
          {busy ? (
            <button type="button" className="vw-btn-secondary vw-btn-sm" onClick={() => void handleCancel()} disabled={actionPending}>
              {actionPending ? "Cancelling…" : "Cancel"}
            </button>
          ) : ready && !status.cudaVerified && !status.restartRequired ? (
            <button type="button" className="vw-btn-primary vw-btn-sm" onClick={() => void handleVerify()} disabled={actionPending}>
              {actionPending ? "Verifying…" : "Verify"}
            </button>
          ) : !ready ? (
            <button type="button" className="vw-btn-primary vw-btn-sm" onClick={() => void handleInstall()} disabled={!desktop || loading || actionPending}>
              {actionPending ? "Starting…" : failed ? "Retry" : "Download"}
            </button>
          ) : null}
        </div>
      </div>

      {busy && (
        <div className="mt-4">
          <div className="vw-progress" role="progressbar" aria-label="NVIDIA acceleration download" aria-valuemin={0} aria-valuemax={100} aria-valuenow={percent === null ? undefined : Math.round(percent)}>
            <div className="vw-progress-fill vw-progress-fill-accent" style={{ width: `${Math.max(4, Math.min(100, percent ?? 4))}%` }} />
          </div>
          <p className="mt-1.5 text-[11px] text-faint">
            {downloaded !== null && downloaded !== undefined && total
              ? `${formatBytes(downloaded)} of ${formatBytes(total)}${percent === null ? "" : ` · ${Math.round(percent)}%`}`
              : status?.state === "verifying"
                ? "Checking GPU support…"
                : status?.state === "installing"
                  ? "Finishing setup…"
                  : "Preparing download…"}
          </p>
        </div>
      )}

      {error && <p className="mt-3 rounded-xl border border-status-danger-border bg-status-danger-bg px-3 py-2 text-xs text-status-danger-text" role="alert">{error}</p>}
    </section>
  );
}
