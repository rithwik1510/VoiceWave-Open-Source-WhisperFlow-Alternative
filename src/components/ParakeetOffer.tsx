import type { ModelCatalogItem, ModelStatus } from "../types/voicewave";
import { ModelDownloadProgress } from "./ModelDownloadProgress";

interface ParakeetOfferProps {
  /** Parakeet's catalog row on this PC. */
  model: ModelCatalogItem;
  /** Parakeet's download status; undefined until it starts. */
  status: ModelStatus | undefined;
  /** Download Parakeet, then make it active once it installed. */
  onSwitch: () => void;
  onCancel: () => void;
  /** "Not now": the card never shows again. */
  onDecline: () => void;
}

const toMb = (bytes: number) => `${Math.round(bytes / (1024 * 1024))} MB`;

/** Home view card, shown once, offering existing users the switch to
 * Parakeet (the default for new installs since 0.8.0). The current model
 * keeps dictating until Parakeet installed; a failed or cancelled download
 * shows the usual Retry/Resume here and leaves the current model active. */
export function ParakeetOffer({ model, status, onSwitch, onCancel, onDecline }: ParakeetOfferProps) {
  const downloading = status?.state === "downloading";
  const stopped = status?.state === "failed" || status?.state === "cancelled";

  return (
    <section
      className="mx-auto mb-6 max-w-6xl rounded-2xl border border-edge bg-inset px-5 py-4"
      aria-label="Parakeet is the new default speech model"
    >
      <div className="flex min-w-0 items-start gap-3">
        <span className="mt-1.5 inline-block h-2 w-2 flex-none rounded-full bg-accent" aria-hidden="true" />
        <div className="min-w-0">
          <p className="text-sm font-semibold text-ink-strong">Parakeet is VoiceWave's new default speech model</p>
          <div className="mt-1 max-w-2xl space-y-1 text-sm text-faint">
            <p>It's faster than your current model, and your dictionary words come out right more often.</p>
            <p>
              Switching is a one-time {toMb(model.sizeBytes)} download. Parakeet runs on this PC, like your current
              model does now.
            </p>
            <p>
              Your current model keeps working until Parakeet is ready, and you can switch back anytime in Models.
            </p>
          </div>
          {!downloading && !stopped && (
            <div className="mt-3 flex flex-wrap items-center gap-2">
              <button type="button" className="vw-btn-primary vw-btn-sm" onClick={onSwitch}>
                Switch to Parakeet
              </button>
              <button type="button" className="vw-btn-secondary vw-btn-sm" onClick={onDecline}>
                Not now
              </button>
            </div>
          )}
        </div>
      </div>
      {(downloading || stopped) && (
        <div className="mt-4">
          <ModelDownloadProgress status={status} onRetry={onSwitch} onCancel={onCancel} />
          {stopped && (
            <button type="button" className="vw-btn-secondary vw-btn-sm mt-3" onClick={onDecline}>
              Not now
            </button>
          )}
        </div>
      )}
    </section>
  );
}
