import { X } from "lucide-react";

import type { ModelOffer, ModelStatus, ModelTier } from "../types/voicewave";
import { ModelDownloadProgress } from "./ModelDownloadProgress";

interface CpuModelNoticeProps {
  /** The processor model to switch to (`SpeechEngineStatus.cpuModelOffer`). */
  offer: ModelOffer;
  /** Tier of the graphics-card model stuck on the processor. */
  activeTier: ModelTier | null;
  /** The user turned GPU acceleration off (vs. the graphics card being unusable). */
  gpuOff: boolean;
  /** Download status of the offered model; undefined until it starts. */
  status: ModelStatus | undefined;
  /** Download the offered model; the active model stays as it is. */
  onDownload: () => void;
  onCancel: () => void;
  onDismiss: () => void;
}

const toMb = (bytes: number) => `${Math.round(bytes / (1024 * 1024))} MB`;

/** Home view prompt for a graphics-card model stuck on the processor (GPU
 * check failed, cooling down, gone, or acceleration off) with no processor
 * model to stand in: every dictation runs on the CPU worker, 14-23 s each
 * for Large Turbo. One button downloads processor Small, which then stands
 * in on the processor by itself (the backend picks an installed
 * faster-whisper model as the CPU rung at every dictation); the active model
 * keeps the GPU once it is usable again. */
export function CpuModelNotice({
  offer,
  activeTier,
  gpuOff,
  status,
  onDownload,
  onCancel,
  onDismiss
}: CpuModelNoticeProps) {
  const downloadVisible =
    status?.state === "downloading" || status?.state === "failed" || status?.state === "cancelled";
  const reason = gpuOff
    ? "GPU acceleration is off"
    : "VoiceWave can't use your graphics card right now";
  const detail =
    activeTier === "largeTurbo"
      ? `${reason}, so Large Turbo runs on your processor: about 15–25 seconds per dictation. Small for processors is much faster.`
      : `${reason}, so the graphics card version of Small runs on your processor, slower than it needs to be. Small for processors is much faster.`;

  return (
    <section
      className="mx-auto mb-6 max-w-6xl rounded-2xl border border-edge bg-inset px-5 py-4"
      role="status"
      aria-label="Dictation is slow on this PC's processor"
    >
      <div className="flex items-start justify-between gap-4">
        <div className="flex min-w-0 items-start gap-3">
          <span className="mt-1.5 inline-block h-2 w-2 flex-none rounded-full bg-accent" aria-hidden="true" />
          <div className="min-w-0">
            <p className="text-sm font-semibold text-ink-strong">Dictation is slow on this PC's processor</p>
            <p className="mt-1 max-w-2xl text-sm text-faint">{detail}</p>
            {!downloadVisible && (
              <div className="mt-3 flex flex-wrap items-center gap-3">
                <button type="button" className="vw-btn-primary vw-btn-sm" onClick={onDownload}>
                  {`Get Small for processors (${toMb(offer.sizeBytes)})`}
                </button>
                <p className="text-xs text-hint">
                  Used by itself whenever your graphics card isn't. Your current model stays.
                </p>
              </div>
            )}
          </div>
        </div>
        <button type="button" className="vw-icon-btn" aria-label="Dismiss" title="Dismiss for now" onClick={onDismiss}>
          <X size={14} />
        </button>
      </div>
      {downloadVisible && (
        <div className="mt-4">
          <ModelDownloadProgress status={status} onRetry={onDownload} onCancel={onCancel} />
        </div>
      )}
    </section>
  );
}
