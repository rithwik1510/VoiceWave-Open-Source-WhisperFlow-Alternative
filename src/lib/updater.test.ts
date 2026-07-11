import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  canUseTauri: vi.fn(),
  check: vi.fn(),
  relaunch: vi.fn()
}));

vi.mock("./tauri", () => ({ canUseTauri: mocks.canUseTauri }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: mocks.check }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: mocks.relaunch }));

import { checkForUpdate, installPendingUpdate } from "./updater";

describe("updater bridge", () => {
  beforeEach(async () => {
    mocks.canUseTauri.mockReset();
    mocks.check.mockReset();
    mocks.relaunch.mockReset();
    mocks.canUseTauri.mockReturnValue(false);
    // Clear module-level pending state between tests.
    await checkForUpdate();
  });

  it("does nothing in the browser fallback", async () => {
    await expect(checkForUpdate()).resolves.toBeNull();
    expect(mocks.check).not.toHaveBeenCalled();
  });

  it("returns a verified Tauri update and clears stale state when none remains", async () => {
    mocks.canUseTauri.mockReturnValue(true);
    const update = {
      version: "0.5.5",
      currentVersion: "0.5.4",
      date: "2026-07-11",
      body: "Reliability fixes"
    };
    mocks.check.mockResolvedValueOnce(update).mockResolvedValueOnce(null);

    await expect(checkForUpdate()).resolves.toEqual({
      version: "0.5.5",
      currentVersion: "0.5.4",
      date: "2026-07-11",
      notes: "Reliability fixes"
    });
    await expect(checkForUpdate()).resolves.toBeNull();
    await expect(installPendingUpdate()).rejects.toThrow("No update is queued");
  });

  it("installs the queued update, reports progress, and relaunches", async () => {
    mocks.canUseTauri.mockReturnValue(true);
    const downloadAndInstall = vi.fn(async (onEvent) => {
      onEvent({ event: "Started", data: { contentLength: 10 } });
      onEvent({ event: "Progress", data: { chunkLength: 4 } });
      onEvent({ event: "Finished", data: {} });
    });
    mocks.check.mockResolvedValue({
      version: "0.5.5",
      currentVersion: "0.5.4",
      downloadAndInstall
    });
    const progress = vi.fn();

    await checkForUpdate();
    await installPendingUpdate(progress);

    expect(progress).toHaveBeenNthCalledWith(1, { phase: "started", contentLength: 10 });
    expect(progress).toHaveBeenNthCalledWith(2, {
      phase: "progress",
      downloaded: 4,
      contentLength: 10
    });
    expect(progress).toHaveBeenNthCalledWith(3, { phase: "finished" });
    expect(mocks.relaunch).toHaveBeenCalledOnce();
  });
});
