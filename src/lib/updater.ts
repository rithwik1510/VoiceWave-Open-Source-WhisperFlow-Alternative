import type { Update } from "@tauri-apps/plugin-updater";

import type { UpdateErrorKind, UpdateFailureStage } from "../types/voicewave";
import { canUseTauri, clearUpdateInstalling, markUpdateInstalling, track } from "./tauri";

export interface AvailableUpdate {
  /** Version offered by the release `latest.json` (e.g. "0.4.0"). */
  version: string;
  /** Version currently installed (from tauri.conf.json at build time). */
  currentVersion: string;
  /** Release date string, if the manifest provides one. */
  date?: string;
  /** Release notes / changelog body, if the manifest provides one. */
  notes?: string;
}

export type DownloadProgress =
  | { phase: "started"; contentLength?: number }
  | { phase: "progress"; downloaded: number; contentLength?: number }
  | { phase: "finished" };

// The `Update` handle returned by `check()` carries the verified download URL
// and signature. We hold it here between the "check" and the "install" steps so
// the UI only has to pass around plain, serialisable info.
let pendingUpdate: Update | null = null;

/**
 * Ask the configured GitHub endpoint whether a newer signed release exists.
 * Returns `null` (and never throws for the common offline / up-to-date cases)
 * so callers can treat "no update" and "could not check" the same way.
 */
export async function checkForUpdate(): Promise<AvailableUpdate | null> {
  if (!canUseTauri()) {
    return null;
  }

  const { check } = await import("@tauri-apps/plugin-updater");
  const update = await check();
  if (!update) {
    pendingUpdate = null;
    return null;
  }

  pendingUpdate = update;
  return {
    version: update.version,
    currentVersion: update.currentVersion,
    date: update.date ?? undefined,
    notes: update.body ?? undefined
  };
}

/**
 * Buckets an updater error for the `update_failed` analytics event (plan 024
 * 4.3). The message is matched here and then dropped; it's never sent.
 */
export function classifyUpdateFailure(
  error: unknown,
  downloadFinished: boolean
): { stage: UpdateFailureStage; errorKind: UpdateErrorKind } {
  const message = String(error instanceof Error ? error.message : error).toLowerCase();
  const has = (...needles: string[]) => needles.some((needle) => message.includes(needle));
  const stage: UpdateFailureStage = has("signature", "minisign")
    ? "verify"
    : downloadFinished
      ? "install"
      : "download";
  const errorKind: UpdateErrorKind = has("network", "request", "dns", "timed out", "connection")
    ? "network"
    : has("signature")
      ? "signature"
      : has("space", "disk", "os error 112")
        ? "disk"
        : has("access is denied", "os error 5")
          ? "permission"
          : "other";
  return { stage, errorKind };
}

/**
 * Download + install the update queued by the most recent {@link checkForUpdate}
 * call, then relaunch into the new version. On Windows this runs the new NSIS
 * setup in `passive` mode (progress UI, no prompts), and the plugin exits the
 * app as the installer starts.
 */
export async function installPendingUpdate(
  onProgress?: (progress: DownloadProgress) => void
): Promise<void> {
  if (!pendingUpdate) {
    throw new Error("No update is queued. Call checkForUpdate() first.");
  }

  const targetVersion = pendingUpdate.version;
  let downloaded = 0;
  let contentLength: number | undefined;
  let downloadFinished = false;

  // The next launch compares its version against this target, so an update
  // that never lands is still counted (plan 024 section 5).
  await markUpdateInstalling(targetVersion);
  try {
    await pendingUpdate.downloadAndInstall((event) => {
      switch (event.event) {
        case "Started":
          contentLength = event.data.contentLength;
          onProgress?.({ phase: "started", contentLength });
          break;
        case "Progress":
          downloaded += event.data.chunkLength;
          onProgress?.({ phase: "progress", downloaded, contentLength });
          break;
        case "Finished":
          downloadFinished = true;
          onProgress?.({ phase: "finished" });
          break;
      }
    });
  } catch (error) {
    await clearUpdateInstalling();
    void track({ type: "update_failed", ...classifyUpdateFailure(error, downloadFinished), targetVersion });
    throw error;
  }

  const { relaunch } = await import("@tauri-apps/plugin-process");
  await relaunch();
}
