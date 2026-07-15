import { beforeEach, describe, expect, it, vi } from "vitest";

const authMocks = vi.hoisted(() => ({
  createUserWithEmailAndPassword: vi.fn(),
  signInWithEmailAndPassword: vi.fn(),
  getRedirectResult: vi.fn(),
  signInWithPopup: vi.fn(),
  signInWithRedirect: vi.fn(),
  signOut: vi.fn(),
  sendPasswordResetEmail: vi.fn(),
  onAuthStateChanged: vi.fn(),
  updateProfile: vi.fn(),
  GoogleAuthProvider: vi.fn().mockImplementation(() => ({
    setCustomParameters: vi.fn()
  }))
}));

const firestoreMocks = vi.hoisted(() => ({
  collection: vi.fn((...args) => ({ type: "collection", args })),
  doc: vi.fn((...args) => ({ type: "doc", args, id: "row-1", ref: {} })),
  query: vi.fn((...args) => ({ type: "query", args })),
  where: vi.fn((...args) => ({ type: "where", args })),
  orderBy: vi.fn((...args) => ({ type: "orderBy", args })),
  limit: vi.fn((value) => ({ type: "limit", value })),
  setDoc: vi.fn(),
  getDoc: vi.fn(),
  getDocs: vi.fn(),
  deleteDoc: vi.fn(),
  batchSet: vi.fn(),
  batchDelete: vi.fn(),
  batchCommit: vi.fn(),
  writeBatch: vi.fn()
}));

vi.mock("firebase/auth", () => ({
  createUserWithEmailAndPassword: authMocks.createUserWithEmailAndPassword,
  signInWithEmailAndPassword: authMocks.signInWithEmailAndPassword,
  getRedirectResult: authMocks.getRedirectResult,
  signInWithPopup: authMocks.signInWithPopup,
  signInWithRedirect: authMocks.signInWithRedirect,
  signOut: authMocks.signOut,
  sendPasswordResetEmail: authMocks.sendPasswordResetEmail,
  onAuthStateChanged: authMocks.onAuthStateChanged,
  updateProfile: authMocks.updateProfile,
  GoogleAuthProvider: authMocks.GoogleAuthProvider
}));

vi.mock("firebase/firestore", () => ({
  collection: firestoreMocks.collection,
  deleteDoc: firestoreMocks.deleteDoc,
  doc: firestoreMocks.doc,
  getDoc: firestoreMocks.getDoc,
  getDocs: firestoreMocks.getDocs,
  limit: firestoreMocks.limit,
  orderBy: firestoreMocks.orderBy,
  query: firestoreMocks.query,
  setDoc: firestoreMocks.setDoc,
  where: firestoreMocks.where,
  writeBatch: firestoreMocks.writeBatch
}));

vi.mock("./firebase", () => ({
  firebaseEnabled: true,
  getFirebase: vi.fn(async () => ({ auth: {}, db: {} }))
}));

import {
  dictionaryDocumentId,
  listCloudDictionaryRecords,
  listCloudVoiceSnippetRecords,
  saveCloudSentence,
  signInCloud,
  snippetDocumentId,
  upsertCloudDictionaryRecords,
  upsertCloudVoiceSnippetRecords
} from "./cloudSync";

describe("cloudSync", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    firestoreMocks.writeBatch.mockImplementation(() => ({
      set: firestoreMocks.batchSet,
      delete: firestoreMocks.batchDelete,
      commit: firestoreMocks.batchCommit
    }));
    firestoreMocks.batchCommit.mockResolvedValue(undefined);
  });

  it("maps auth errors to CloudSyncError with stable fields", async () => {
    authMocks.signInWithEmailAndPassword.mockRejectedValueOnce({
      code: "auth/invalid-credential",
      message: "bad creds"
    });

    await expect(signInCloud("a@b.com", "bad")).rejects.toMatchObject({
      name: "CloudSyncError",
      code: "auth/invalid-credential",
      retryable: false,
      context: "signin",
      message: "Invalid email or password."
    });
  });

  it("deduplicates rapid duplicate sentence writes", async () => {
    const docsPayload = [
      {
        id: "row-1",
        ref: {},
        data: () => ({ text: "hello", createdAtUtcMs: 1700000000000 })
      }
    ];
    firestoreMocks.getDocs.mockResolvedValue({ docs: docsPayload, empty: false });

    const first = await saveCloudSentence("uid-1", "hello");
    const second = await saveCloudSentence("uid-1", "hello");

    expect(first).toHaveLength(1);
    expect(second).toHaveLength(1);
    expect(firestoreMocks.setDoc).toHaveBeenCalledTimes(1);
  });

  it("does not retry permission-denied sentence writes", async () => {
    firestoreMocks.setDoc.mockRejectedValueOnce({
      code: "permission-denied",
      message: "rules rejected"
    });

    await expect(saveCloudSentence("uid-1", "blocked write")).rejects.toMatchObject({
      name: "CloudSyncError",
      code: "permission-denied",
      retryable: false,
      context: "save-sentence",
      message: "Cloud write blocked by server policy. Check account and payload constraints."
    });
    expect(firestoreMocks.setDoc).toHaveBeenCalledTimes(1);
  });

  it("maps legacy dictionary rows without deleting them during reads", async () => {
    firestoreMocks.getDocs.mockResolvedValue({
      docs: [{
        id: "legacy-random-id",
        data: () => ({
          term: "VoiceWave",
          source: "manual-add",
          termNormalized: "voicewave",
          createdAtUtcMs: 1_700_000_000_000
        })
      }]
    });

    const snapshot = await listCloudDictionaryRecords("uid-legacy");

    expect(snapshot).toEqual({
      records: [{
        term: "VoiceWave",
        normalizedTerm: "voicewave",
        source: "manual-add",
        createdAtUtcMs: 1_700_000_000_000,
        updatedAtUtcMs: 1_700_000_000_000,
        deletedAtUtcMs: null
      }],
      legacyIds: ["legacy-random-id"],
      deterministicIdentities: []
    });
    expect(firestoreMocks.batchDelete).not.toHaveBeenCalled();
  });

  it("recomputes stale legacy identities and quarantines unsyncable rows", async () => {
    firestoreMocks.getDocs.mockResolvedValue({
      docs: [
        {
          // Old normalization kept interior double spaces; the current
          // contract collapses them. The stored value must be recomputed.
          id: "legacy-double-space",
          data: () => ({
            term: "Voice  Wave",
            source: "manual-add",
            termNormalized: "voice  wave",
            createdAtUtcMs: 1_700_000_000_000
          })
        },
        {
          // A term the local contract can never accept must be skipped, not
          // allowed to abort every future reconciliation.
          id: "poison-control-char",
          data: () => ({
            term: "bad\nterm",
            source: "manual-add",
            termNormalized: "bad\nterm",
            createdAtUtcMs: 1_700_000_000_000
          })
        }
      ]
    });

    const snapshot = await listCloudDictionaryRecords("uid-stale");

    expect(snapshot.records).toHaveLength(1);
    expect(snapshot.records[0].normalizedTerm).toBe("voice wave");
    expect(snapshot.legacyIds).toEqual(["legacy-double-space"]);
    expect(snapshot.deterministicIdentities).toEqual([]);
  });

  it("uses deterministic encoded IDs and chunks dictionary batches at 500 writes", async () => {
    const records = Array.from({ length: 501 }, (_, index) => ({
      term: index === 0 ? "../ Voice Term" : `Term ${index}`,
      normalizedTerm: index === 0 ? "../ voice term" : `term ${index}`,
      source: "manual-add",
      createdAtUtcMs: 1_700_000_000_000 + index,
      updatedAtUtcMs: 1_700_000_000_000 + index,
      deletedAtUtcMs: index === 500 ? 1_700_000_000_500 : null
    }));

    await upsertCloudDictionaryRecords("uid-batch", records);

    expect(firestoreMocks.writeBatch).toHaveBeenCalledTimes(2);
    expect(firestoreMocks.batchCommit).toHaveBeenCalledTimes(2);
    expect(firestoreMocks.batchSet).toHaveBeenCalledTimes(501);
    expect(dictionaryDocumentId("../ voice term")).toBe("term-..%2F%20voice%20term");
    expect(firestoreMocks.doc).toHaveBeenCalledWith(
      {},
      "users",
      "uid-batch",
      "dictionaryTerms",
      "term-..%2F%20voice%20term"
    );
    expect(firestoreMocks.batchSet).toHaveBeenLastCalledWith(
      expect.anything(),
      expect.objectContaining({ deletedAtUtcMs: 1_700_000_000_500 })
    );
  });

  it("rejects a dictionary record whose TypeScript identity disagrees with Rust", async () => {
    await expect(upsertCloudDictionaryRecords("uid-mismatch", [{
      term: "VoiceWave",
      normalizedTerm: "wrong",
      source: "manual-add",
      createdAtUtcMs: 1_700_000_000_000,
      updatedAtUtcMs: 1_700_000_000_000,
      deletedAtUtcMs: null
    }])).rejects.toMatchObject({ code: "dictionary-identity-mismatch", retryable: false });
    expect(firestoreMocks.writeBatch).not.toHaveBeenCalled();
  });

  it("reads valid multiline snippets and quarantines malformed rows without deleting them", async () => {
    firestoreMocks.getDocs.mockResolvedValue({
      docs: [
        {
          id: snippetDocumentId("support reply"),
          data: () => ({
            trigger: "Support reply",
            normalizedTrigger: "support reply",
            expansion: "Hello,\n\tThanks for writing.",
            createdAtUtcMs: 1_700_000_000_000,
            updatedAtUtcMs: 1_700_000_000_100,
            deletedAtUtcMs: null
          })
        },
        {
          id: snippetDocumentId("deleted reply"),
          data: () => ({
            trigger: "Deleted reply",
            normalizedTrigger: "deleted reply",
            expansion: "",
            createdAtUtcMs: 1_700_000_000_000,
            updatedAtUtcMs: 1_700_000_000_100,
            deletedAtUtcMs: 1_700_000_000_200
          })
        },
        {
          id: "poison-control",
          data: () => ({
            trigger: "Bad reply",
            normalizedTrigger: "bad reply",
            expansion: "bad\u000btext",
            createdAtUtcMs: 1_700_000_000_000,
            updatedAtUtcMs: 1_700_000_000_100,
            deletedAtUtcMs: null
          })
        },
        {
          id: "poison-empty",
          data: () => ({
            trigger: "Empty reply",
            normalizedTrigger: "empty reply",
            expansion: " \n\t ",
            createdAtUtcMs: 1_700_000_000_000,
            updatedAtUtcMs: 1_700_000_000_100,
            deletedAtUtcMs: null
          })
        },
        {
          id: "poison-marker",
          data: () => ({
            trigger: "Marker reply",
            normalizedTrigger: "marker reply",
            expansion: "bad\uE000text",
            createdAtUtcMs: 1_700_000_000_000,
            updatedAtUtcMs: 1_700_000_000_100,
            deletedAtUtcMs: null
          })
        },
        {
          id: "poison-reserved-trigger",
          data: () => ({
            trigger: "New line",
            normalizedTrigger: "new line",
            expansion: "Should be quarantined",
            createdAtUtcMs: 1_700_000_000_000,
            updatedAtUtcMs: 1_700_000_000_100,
            deletedAtUtcMs: null
          })
        },
        {
          id: "poison-future-timestamp",
          data: () => ({
            trigger: "Future reply",
            normalizedTrigger: "future reply",
            expansion: "This row must remain quarantined.",
            createdAtUtcMs: Date.now() + 5 * 60 * 1_000 + 1,
            updatedAtUtcMs: Date.now() + 5 * 60 * 1_000 + 1,
            deletedAtUtcMs: null
          })
        },
        {
          // Rust reconciliation rejects an identity mismatch fail-closed, so
          // letting this row through would permanently abort every sync.
          id: "poison-identity-mismatch",
          data: () => ({
            trigger: "Mismatch reply",
            normalizedTrigger: "some other identity",
            expansion: "Quarantined, not synced.",
            createdAtUtcMs: 1_700_000_000_000,
            updatedAtUtcMs: 1_700_000_000_100,
            deletedAtUtcMs: null
          })
        }
      ]
    });

    const snapshot = await listCloudVoiceSnippetRecords("uid-snippet-read");

    expect(snapshot.records).toHaveLength(2);
    expect(snapshot.records[0].expansion).toBe("Hello,\n\tThanks for writing.");
    expect(snapshot.records[1]).toMatchObject({ expansion: "", deletedAtUtcMs: 1_700_000_000_200 });
    expect(snapshot.deterministicIdentities).toEqual(["support reply", "deleted reply"]);
    expect(firestoreMocks.batchDelete).not.toHaveBeenCalled();
    expect(firestoreMocks.deleteDoc).not.toHaveBeenCalled();
  });

  it("uses deterministic encoded IDs and chunks snippet batches at 500 writes", async () => {
    const records = Array.from({ length: 501 }, (_, index) => ({
      trigger: index === 0 ? "../ Work email" : `Snippet ${index}`,
      normalizedTrigger: index === 0 ? "../ work email" : `snippet ${index}`,
      expansion: index === 500 ? "" : `Exact expansion ${index}`,
      createdAtUtcMs: 1_700_001_000_000 + index,
      updatedAtUtcMs: 1_700_001_000_000 + index,
      deletedAtUtcMs: index === 500 ? 1_700_001_000_500 : null
    }));

    await upsertCloudVoiceSnippetRecords("uid-snippet-batch", records);

    expect(firestoreMocks.writeBatch).toHaveBeenCalledTimes(2);
    expect(firestoreMocks.batchCommit).toHaveBeenCalledTimes(2);
    expect(firestoreMocks.batchSet).toHaveBeenCalledTimes(501);
    expect(snippetDocumentId("../ work email")).toBe("snippet-..%2F%20work%20email");
    expect(firestoreMocks.doc).toHaveBeenCalledWith(
      {},
      "users",
      "uid-snippet-batch",
      "voiceSnippets",
      "snippet-..%2F%20work%20email"
    );
  });

  it("rejects mismatched or invalid local snippet records before opening a batch", async () => {
    const base = {
      trigger: "Work email",
      normalizedTrigger: "wrong",
      expansion: "person@example.com",
      createdAtUtcMs: 1_700_002_000_000,
      updatedAtUtcMs: 1_700_002_000_000,
      deletedAtUtcMs: null
    };
    await expect(upsertCloudVoiceSnippetRecords("uid-snippet-mismatch", [base]))
      .rejects.toMatchObject({ code: "snippet-identity-mismatch", retryable: false });
    await expect(upsertCloudVoiceSnippetRecords("uid-snippet-control", [{
      ...base,
      normalizedTrigger: "work email",
      expansion: "bad\rtext"
    }])).rejects.toMatchObject({ code: "snippet-validation", retryable: false });
    await expect(upsertCloudVoiceSnippetRecords("uid-snippet-marker", [{
      ...base,
      normalizedTrigger: "work email",
      expansion: "bad\uE001text"
    }])).rejects.toMatchObject({ code: "snippet-validation", retryable: false });
    await expect(upsertCloudVoiceSnippetRecords("uid-snippet-reserved", [{
      ...base,
      trigger: "New line",
      normalizedTrigger: "new line"
    }])).rejects.toMatchObject({ code: "snippet-validation", retryable: false });
    expect(firestoreMocks.writeBatch).not.toHaveBeenCalled();
  });

  it("returns before cloud setup for an empty snippet upsert", async () => {
    await expect(upsertCloudVoiceSnippetRecords("uid-snippet-empty", [])).resolves.toBeUndefined();
    expect(firestoreMocks.writeBatch).not.toHaveBeenCalled();
  });

  it("deduplicates snippet writes without exposing private content in diagnostics", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(1_800_000_000_000);
    const info = vi.spyOn(console, "info").mockImplementation(() => undefined);
    const record = {
      trigger: "Private phrase",
      normalizedTrigger: "private phrase",
      expansion: "secret@example.com",
      createdAtUtcMs: 1_700_003_000_000,
      updatedAtUtcMs: 1_700_003_000_000,
      deletedAtUtcMs: null
    };

    await upsertCloudVoiceSnippetRecords("uid-snippet-dedup", [record]);
    await upsertCloudVoiceSnippetRecords("uid-snippet-dedup", [record]);
    vi.advanceTimersByTime(501);
    await upsertCloudVoiceSnippetRecords("uid-snippet-dedup", [{
      ...record,
      expansion: "different-secret@example.com"
    }]);

    expect(firestoreMocks.batchSet).toHaveBeenCalledTimes(2);
    expect(JSON.stringify(info.mock.calls)).not.toContain(record.trigger);
    expect(JSON.stringify(info.mock.calls)).not.toContain(record.expansion);
    info.mockRestore();
    vi.useRealTimers();
  });
});
