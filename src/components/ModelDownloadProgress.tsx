import type { ModelStatus } from "../types/voicewave";

interface ModelDownloadProgressProps {
  /** Status of the model being downloaded; undefined until the first event. */
  status: ModelStatus | undefined;
  onRetry: () => void;
  onCancel: () => void;
}

const toMb = (bytes: number) => Math.round(bytes / (1024 * 1024));

/** Progress card for the one-time speech model download, shared by
 * onboarding and the model gate: percent, bar, "X MB of Y MB", Cancel while
 * running, and Retry/Resume once it failed or was cancelled. */
export function ModelDownloadProgress({ status, onRetry, onCancel }: ModelDownloadProgressProps) {
  const failed = status?.state === "failed";
  const cancelled = status?.state === "cancelled";
  const stopped = failed || cancelled;
  const progress = Math.max(0, Math.min(100, status?.progress ?? 0));

  // A bare percent hides how much of a ~487 MB download is left (and stalls
  // read as "nothing is happening"). Show the bytes once any have landed;
  // before that the backend message says what it is doing ("Preparing...").
  const downloadedLabel =
    typeof status?.downloadedBytes === "number" &&
    status.downloadedBytes > 0 &&
    typeof status.totalBytes === "number" &&
    status.totalBytes > 0
      ? `${toMb(status.downloadedBytes)} MB of ${toMb(status.totalBytes)} MB`
      : null;

  return (
    <div className="rounded-2xl border border-edge bg-inset px-5 py-5">
      <div className="flex items-center justify-between gap-3">
        <p className="text-sm font-semibold text-ink-strong">
          {failed
            ? "The model download hit a snag."
            : cancelled
              ? "Download cancelled."
              : "Downloading the speech model…"}
        </p>
        {stopped ? (
          <button type="button" className="vw-btn-primary vw-btn-sm" onClick={onRetry}>
            {failed ? "Retry" : "Resume"}
          </button>
        ) : (
          <span className="text-sm tabular-nums text-accent-text">{progress}%</span>
        )}
      </div>
      {!stopped && (
        <div className="vw-onb-progress mt-3">
          <div className="vw-onb-progress-fill" style={{ width: `${Math.max(4, progress)}%` }} />
        </div>
      )}
      <div className="mt-2 flex items-center justify-between gap-3">
        <p
          className={`text-xs ${failed ? "text-status-danger-text" : "tabular-nums text-hint"}`}
          aria-live="polite"
        >
          {stopped ? status?.message : (downloadedLabel ?? status?.message)}
        </p>
        {!stopped && (
          <button
            type="button"
            className="shrink-0 text-xs font-medium text-hint transition-colors hover:text-quiet"
            onClick={onCancel}
          >
            Cancel
          </button>
        )}
      </div>
    </div>
  );
}
