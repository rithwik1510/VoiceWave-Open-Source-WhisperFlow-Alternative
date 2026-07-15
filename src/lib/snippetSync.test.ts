import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  listCloudVoiceSnippetRecords: vi.fn(),
  upsertCloudVoiceSnippetRecords: vi.fn(),
  reconcileVoiceSnippetRecords: vi.fn()
}));

vi.mock("./cloudSync", () => ({
  listCloudVoiceSnippetRecords: mocks.listCloudVoiceSnippetRecords,
  upsertCloudVoiceSnippetRecords: mocks.upsertCloudVoiceSnippetRecords
}));

vi.mock("./tauri", () => ({
  reconcileVoiceSnippetRecords: mocks.reconcileVoiceSnippetRecords
}));

import { syncVoiceSnippetsWithCloud } from "./snippetSync";

const remoteRecord = {
  trigger: "Work email",
  normalizedTrigger: "work email",
  expansion: "person@example.com",
  createdAtUtcMs: 1_700_000_000_000,
  updatedAtUtcMs: 1_700_000_000_100,
  deletedAtUtcMs: null
};

describe("syncVoiceSnippetsWithCloud", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.upsertCloudVoiceSnippetRecords.mockResolvedValue(undefined);
  });

  it("returns Rust-owned active snippets and skips unchanged deterministic records", async () => {
    const snippets = [{
      snippetId: "local-1",
      trigger: remoteRecord.trigger,
      normalizedTrigger: remoteRecord.normalizedTrigger,
      expansion: remoteRecord.expansion,
      createdAtUtcMs: remoteRecord.createdAtUtcMs,
      updatedAtUtcMs: remoteRecord.updatedAtUtcMs
    }];
    mocks.listCloudVoiceSnippetRecords.mockResolvedValue({
      records: [remoteRecord],
      deterministicIdentities: [remoteRecord.normalizedTrigger]
    });
    mocks.reconcileVoiceSnippetRecords.mockResolvedValue({
      snippets,
      records: [remoteRecord],
      limitExceeded: false
    });

    await expect(syncVoiceSnippetsWithCloud("uid-1")).resolves.toEqual({
      snippets,
      records: [remoteRecord],
      limitExceeded: false
    });
    expect(mocks.reconcileVoiceSnippetRecords).toHaveBeenCalledWith([remoteRecord]);
    expect(mocks.upsertCloudVoiceSnippetRecords).toHaveBeenCalledWith("uid-1", []);
  });

  it("upserts only winners whose content changed or deterministic document is absent", async () => {
    const changed = { ...remoteRecord, expansion: "new@example.com", updatedAtUtcMs: 1_700_000_000_200 };
    const localOnly = {
      trigger: "Support reply",
      normalizedTrigger: "support reply",
      expansion: "Hello!",
      createdAtUtcMs: 1_700_000_000_300,
      updatedAtUtcMs: 1_700_000_000_300,
      deletedAtUtcMs: null
    };
    mocks.listCloudVoiceSnippetRecords.mockResolvedValue({
      records: [remoteRecord],
      deterministicIdentities: [remoteRecord.normalizedTrigger]
    });
    mocks.reconcileVoiceSnippetRecords.mockResolvedValue({
      snippets: [],
      records: [changed, localOnly],
      limitExceeded: false
    });

    await syncVoiceSnippetsWithCloud("uid-2");

    expect(mocks.upsertCloudVoiceSnippetRecords).toHaveBeenCalledWith("uid-2", [changed, localOnly]);
  });

  it("stops a stale auth session before local reconciliation or cloud writes", async () => {
    let current = true;
    mocks.listCloudVoiceSnippetRecords.mockImplementation(async () => {
      current = false;
      return { records: [remoteRecord], deterministicIdentities: [] };
    });

    await expect(syncVoiceSnippetsWithCloud("uid-old", () => current))
      .rejects.toThrow("no longer current");
    expect(mocks.reconcileVoiceSnippetRecords).not.toHaveBeenCalled();
    expect(mocks.upsertCloudVoiceSnippetRecords).not.toHaveBeenCalled();
  });
});
