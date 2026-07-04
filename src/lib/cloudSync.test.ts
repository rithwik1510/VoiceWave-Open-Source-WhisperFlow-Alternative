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
  deleteDoc: vi.fn()
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
  where: firestoreMocks.where
}));

vi.mock("./firebase", () => ({
  firebaseEnabled: true,
  getFirebase: vi.fn(async () => ({ auth: {}, db: {} }))
}));

import { saveCloudSentence, signInCloud } from "./cloudSync";

describe("cloudSync", () => {
  beforeEach(() => {
    vi.clearAllMocks();
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
});
