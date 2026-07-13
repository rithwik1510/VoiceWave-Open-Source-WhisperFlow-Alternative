import { beforeEach, describe, expect, it, vi } from "vitest";

const cloudMocks = vi.hoisted(() => ({
  list: vi.fn(),
  upsert: vi.fn(),
  cleanup: vi.fn()
}));

const tauriMocks = vi.hoisted(() => ({
  reconcile: vi.fn()
}));

vi.mock("./cloudSync", () => ({
  listCloudDictionaryRecords: cloudMocks.list,
  upsertCloudDictionaryRecords: cloudMocks.upsert,
  deleteLegacyCloudDictionaryRecords: cloudMocks.cleanup
}));

vi.mock("./tauri", () => ({
  reconcileDictionaryRecords: tauriMocks.reconcile
}));

import { syncDictionaryWithCloud } from "./dictionarySync";

const remoteRecord = {
  term: "Remote",
  normalizedTerm: "remote",
  source: "legacy",
  createdAtUtcMs: 1_700_000_000_000,
  updatedAtUtcMs: 1_700_000_000_000,
  deletedAtUtcMs: null
};

const winningRecord = {
  ...remoteRecord,
  term: "Local",
  normalizedTerm: "local",
  source: "manual-add",
  updatedAtUtcMs: 1_700_000_000_100
};

describe("dictionarySync", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    cloudMocks.list.mockResolvedValue({
      records: [remoteRecord],
      legacyIds: ["legacy-1"],
      deterministicIdentities: []
    });
    tauriMocks.reconcile.mockResolvedValue({
      terms: [{ termId: "dt-1", term: "Local", source: "manual-add", createdAtUtcMs: 1_700_000_000_000 }],
      records: [winningRecord]
    });
    cloudMocks.upsert.mockResolvedValue(undefined);
    cloudMocks.cleanup.mockResolvedValue(undefined);
  });

  it("delegates all merge decisions to Rust and cleans legacy rows only after upsert", async () => {
    const terms = await syncDictionaryWithCloud("uid-1");

    expect(tauriMocks.reconcile).toHaveBeenCalledWith([remoteRecord]);
    expect(cloudMocks.upsert).toHaveBeenCalledWith("uid-1", [winningRecord]);
    expect(cloudMocks.cleanup).toHaveBeenCalledWith("uid-1", ["legacy-1"]);
    expect(cloudMocks.upsert.mock.invocationCallOrder[0])
      .toBeLessThan(cloudMocks.cleanup.mock.invocationCallOrder[0]);
    expect(terms).toEqual([
      { termId: "dt-1", term: "Local", source: "manual-add", createdAtUtcMs: 1_700_000_000_000 }
    ]);
  });

  it("never deletes legacy rows when deterministic upsert fails", async () => {
    cloudMocks.upsert.mockRejectedValueOnce(new Error("offline"));

    await expect(syncDictionaryWithCloud("uid-1")).rejects.toThrow("offline");
    expect(cloudMocks.cleanup).not.toHaveBeenCalled();
  });

  it("skips rewriting unchanged deterministic documents", async () => {
    const unchanged = {
      term: "Stable",
      normalizedTerm: "stable",
      source: "manual-add",
      createdAtUtcMs: 1_700_000_000_000,
      updatedAtUtcMs: 1_700_000_000_000,
      deletedAtUtcMs: null
    };
    cloudMocks.list.mockResolvedValue({
      records: [unchanged, remoteRecord],
      legacyIds: [],
      deterministicIdentities: ["stable"]
    });
    tauriMocks.reconcile.mockResolvedValue({
      terms: [],
      records: [unchanged, winningRecord]
    });

    await syncDictionaryWithCloud("uid-2");

    expect(cloudMocks.upsert).toHaveBeenCalledWith("uid-2", [winningRecord]);
  });
});
