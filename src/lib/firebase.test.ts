import { beforeEach, afterEach, describe, expect, it, vi } from "vitest";

const firebaseAppMocks = vi.hoisted(() => ({
  getApp: vi.fn(() => ({ name: "voicewave-app" })),
  getApps: vi.fn(() => []),
  initializeApp: vi.fn(() => ({ name: "voicewave-app" }))
}));

const firebaseAuthMocks = vi.hoisted(() => ({
  getAuth: vi.fn(() => ({ kind: "auth" }))
}));

const firestoreMocks = vi.hoisted(() => ({
  getFirestore: vi.fn(() => ({ kind: "firestore" }))
}));

vi.mock("firebase/app", () => ({
  getApp: firebaseAppMocks.getApp,
  getApps: firebaseAppMocks.getApps,
  initializeApp: firebaseAppMocks.initializeApp
}));

vi.mock("firebase/auth", () => ({
  getAuth: firebaseAuthMocks.getAuth
}));

vi.mock("firebase/firestore", () => ({
  getFirestore: firestoreMocks.getFirestore
}));

const FIREBASE_ENV_KEYS = [
  "VITE_FIREBASE_API_KEY",
  "VITE_FIREBASE_AUTH_DOMAIN",
  "VITE_FIREBASE_PROJECT_ID",
  "VITE_FIREBASE_STORAGE_BUCKET",
  "VITE_FIREBASE_MESSAGING_SENDER_ID",
  "VITE_FIREBASE_APP_ID"
] as const;

function stubAllFirebaseConfig(): void {
  vi.stubEnv("VITE_FIREBASE_API_KEY", "key");
  vi.stubEnv("VITE_FIREBASE_AUTH_DOMAIN", "example.firebaseapp.com");
  vi.stubEnv("VITE_FIREBASE_PROJECT_ID", "voicewave-test");
  vi.stubEnv("VITE_FIREBASE_STORAGE_BUCKET", "voicewave-test.appspot.com");
  vi.stubEnv("VITE_FIREBASE_MESSAGING_SENDER_ID", "123456789");
  vi.stubEnv("VITE_FIREBASE_APP_ID", "1:123456789:web:abc123");
}

function clearFirebaseConfig(): void {
  for (const key of FIREBASE_ENV_KEYS) {
    vi.stubEnv(key, "");
  }
}

async function importFirebaseModule() {
  vi.resetModules();
  return import("./firebase");
}

describe("firebase boot gating", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.unstubAllEnvs();
    clearFirebaseConfig();
    vi.stubEnv("VITE_ENABLE_CLOUD_SYNC", "false");
    vi.stubEnv("VITE_ENABLE_FIREBASE_IN_TEST", "false");
  });

  afterEach(() => {
    vi.unstubAllEnvs();
  });

  it("does not boot firebase when cloud sync is disabled", async () => {
    stubAllFirebaseConfig();
    vi.stubEnv("VITE_ENABLE_CLOUD_SYNC", "false");
    vi.stubEnv("VITE_ENABLE_FIREBASE_IN_TEST", "true");

    const module = await importFirebaseModule();

    expect(module.firebaseEnabled).toBe(false);
    expect(firebaseAppMocks.initializeApp).not.toHaveBeenCalled();
    expect(firebaseAuthMocks.getAuth).not.toHaveBeenCalled();
    expect(firestoreMocks.getFirestore).not.toHaveBeenCalled();
  });

  it("boots in test mode only when firebase test override is enabled", async () => {
    stubAllFirebaseConfig();
    vi.stubEnv("VITE_ENABLE_CLOUD_SYNC", "true");
    vi.stubEnv("VITE_ENABLE_FIREBASE_IN_TEST", "false");

    const blockedInTests = await importFirebaseModule();
    expect(blockedInTests.firebaseEnabled).toBe(false);
    // getFirebase() is a no-op when cloud sync is disabled: returns null and
    // never touches the SDK.
    await expect(blockedInTests.getFirebase()).resolves.toBeNull();
    expect(firebaseAppMocks.initializeApp).not.toHaveBeenCalled();

    vi.clearAllMocks();
    vi.stubEnv("VITE_ENABLE_FIREBASE_IN_TEST", "true");

    const enabledInTests = await importFirebaseModule();
    expect(enabledInTests.firebaseEnabled).toBe(true);
    // The SDK is loaded lazily, so importing the module alone must not boot it.
    expect(firebaseAppMocks.initializeApp).not.toHaveBeenCalled();

    const handles = await enabledInTests.getFirebase();
    expect(handles).not.toBeNull();
    expect(firebaseAppMocks.initializeApp).toHaveBeenCalledTimes(1);
    expect(firebaseAuthMocks.getAuth).toHaveBeenCalledTimes(1);
    expect(firestoreMocks.getFirestore).toHaveBeenCalledTimes(1);

    // Subsequent calls reuse the memoized app rather than re-initializing.
    await enabledInTests.getFirebase();
    expect(firebaseAppMocks.initializeApp).toHaveBeenCalledTimes(1);
  });

  it("throws when firebase configuration is partial", async () => {
    clearFirebaseConfig();
    vi.stubEnv("VITE_FIREBASE_API_KEY", "key-only");

    await expect(importFirebaseModule()).rejects.toThrow(
      "Partial Firebase configuration detected. Provide all required VITE_FIREBASE_* variables."
    );
  });
});
