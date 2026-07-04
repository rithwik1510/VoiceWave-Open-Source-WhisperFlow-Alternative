import type { Auth } from "firebase/auth";
import type { Firestore } from "firebase/firestore";

const firebaseConfig = {
  apiKey: import.meta.env.VITE_FIREBASE_API_KEY as string | undefined,
  authDomain: import.meta.env.VITE_FIREBASE_AUTH_DOMAIN as string | undefined,
  projectId: import.meta.env.VITE_FIREBASE_PROJECT_ID as string | undefined,
  storageBucket: import.meta.env.VITE_FIREBASE_STORAGE_BUCKET as string | undefined,
  messagingSenderId: import.meta.env.VITE_FIREBASE_MESSAGING_SENDER_ID as string | undefined,
  appId: import.meta.env.VITE_FIREBASE_APP_ID as string | undefined
};

const isProduction = import.meta.env.MODE === "production";
const hasFirebaseConfig = Object.values(firebaseConfig).every(
  (value) => typeof value === "string" && value.trim().length > 0
);
const hasAnyFirebaseConfig = Object.values(firebaseConfig).some(
  (value) => typeof value === "string" && value.trim().length > 0
);
const isTestMode = import.meta.env.MODE === "test";
const enableFirebaseDuringTests = import.meta.env.VITE_ENABLE_FIREBASE_IN_TEST === "true";
const cloudSyncExplicitlyEnabled = import.meta.env.VITE_ENABLE_CLOUD_SYNC === "true";
const cloudSyncRuntimeEnabled =
  cloudSyncExplicitlyEnabled && (!isTestMode || enableFirebaseDuringTests);

if (isProduction && cloudSyncExplicitlyEnabled && !hasFirebaseConfig) {
  throw new Error(
    "Cloud sync is enabled for production but required VITE_FIREBASE_* variables are missing."
  );
}

if (hasAnyFirebaseConfig && !hasFirebaseConfig) {
  throw new Error(
    "Partial Firebase configuration detected. Provide all required VITE_FIREBASE_* variables."
  );
}

const shouldBootFirebase = cloudSyncRuntimeEnabled && hasFirebaseConfig;

/**
 * Synchronous predicate computed purely from env/config. Callers (e.g. App.tsx)
 * can read this without pulling the Firebase SDK into the main bundle, because
 * the SDK is only ever loaded lazily via {@link getFirebase}.
 */
export const firebaseEnabled = shouldBootFirebase;

export interface FirebaseHandles {
  auth: Auth;
  db: Firestore;
}

let firebasePromise: Promise<FirebaseHandles | null> | null = null;

/**
 * Lazily loads the Firebase SDK (via dynamic import) and initializes the app
 * exactly once. Returns the auth/firestore handles, or `null` when cloud sync
 * is disabled. The dynamic imports keep `firebase/*` out of the main JS chunk.
 */
export function getFirebase(): Promise<FirebaseHandles | null> {
  if (!firebaseEnabled) {
    return Promise.resolve(null);
  }
  if (!firebasePromise) {
    firebasePromise = (async () => {
      const { getApp, getApps, initializeApp } = await import("firebase/app");
      const { getAuth } = await import("firebase/auth");
      const { getFirestore } = await import("firebase/firestore");
      const app = getApps().length > 0 ? getApp() : initializeApp(firebaseConfig);
      return { auth: getAuth(app), db: getFirestore(app) };
    })();
  }
  return firebasePromise;
}
