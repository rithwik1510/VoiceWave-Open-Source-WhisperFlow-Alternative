import type { Update } from "@tauri-apps/plugin-updater";

import { canUseTauri } from "./tauri";

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
 * Download + install the update queued by the most recent {@link checkForUpdate}
 * call, then relaunch into the new version. On Windows this runs the new NSIS
 * setup in `passive` mode (progress UI, no prompts).
 */
export async function installPendingUpdate(
  onProgress?: (progress: DownloadProgress) => void
): Promise<void> {
  if (!pendingUpdate) {
    throw new Error("No update is queued. Call checkForUpdate() first.");
  }

  let downloaded = 0;
  let contentLength: number | undefined;

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
        onProgress?.({ phase: "finished" });
        break;
    }
  });

  const { relaunch } = await import("@tauri-apps/plugin-process");
  await relaunch();
}
