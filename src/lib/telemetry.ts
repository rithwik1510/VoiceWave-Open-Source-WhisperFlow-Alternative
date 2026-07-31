import TelemetryDeck from "@telemetrydeck/sdk";
import { getVersion as getAppVersion } from "@tauri-apps/api/app";

const INSTALLATION_ID_KEY = "voicewave.analytics.installation-id";
const ACTIVATED_KEY = "voicewave.analytics.activated";
const LAST_ACTIVE_DAY_KEY = "voicewave.analytics.last-active-day";
const TELEMETRY_APP_ID = (import.meta.env.VITE_TELEMETRYDECK_APP_ID as string | undefined)?.trim() ?? "";

type SignalClient = {
  signal(type: string, payload?: Record<string, string>): Promise<Response>;
};

type AnonymousUsageDependencies = {
  appId: string;
  storage: Pick<Storage, "getItem" | "setItem"> | null;
  now: () => number;
  randomUUID: () => string;
  getAppVersion: () => Promise<string>;
  createClient: (appId: string, installationId: string) => SignalClient;
};

export type AnonymousUsageResult = {
  configured: boolean;
  activationSent: boolean;
  dailyActiveSent: boolean;
};

let activeClient: { installationId: string; client: SignalClient } | null = null;
let sendQueue: Promise<AnonymousUsageResult> = Promise.resolve({
  configured: false,
  activationSent: false,
  dailyActiveSent: false
});

function createAnonymousId(): string {
  if (typeof globalThis.crypto?.randomUUID === "function") {
    return globalThis.crypto.randomUUID();
  }

  const bytes = new Uint8Array(16);
  globalThis.crypto.getRandomValues(bytes);
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = [...bytes].map((value) => value.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

function defaultDependencies(): AnonymousUsageDependencies {
  return {
    appId: TELEMETRY_APP_ID,
    storage: typeof localStorage === "undefined" ? null : localStorage,
    now: () => Date.now(),
    randomUUID: createAnonymousId,
    getAppVersion,
    createClient: (appId, installationId) => {
      if (activeClient?.installationId === installationId) {
        return activeClient.client;
      }
      const client = new TelemetryDeck({
        appID: appId,
        clientUser: installationId,
        testMode: import.meta.env.DEV
      });
      activeClient = { installationId, client };
      return client;
    }
  };
}

function getOrCreateInstallationId(dependencies: AnonymousUsageDependencies): string {
  const existing = dependencies.storage?.getItem(INSTALLATION_ID_KEY)?.trim();
  if (existing) {
    return existing;
  }

  const installationId = dependencies.randomUUID();
  dependencies.storage?.setItem(INSTALLATION_ID_KEY, installationId);
  return installationId;
}

async function sendAnonymousUsage(
  optedIn: boolean,
  dependencies: AnonymousUsageDependencies
): Promise<AnonymousUsageResult> {
  const result: AnonymousUsageResult = {
    configured: Boolean(dependencies.appId),
    activationSent: false,
    dailyActiveSent: false
  };

  if (!optedIn || !dependencies.appId || !dependencies.storage) {
    return result;
  }

  const installationId = getOrCreateInstallationId(dependencies);
  const client = dependencies.createClient(dependencies.appId, installationId);
  const appVersion = await dependencies.getAppVersion().catch(() => "unknown");
  const payload = {
    "VoiceWave.App.version": appVersion,
    "VoiceWave.Distribution.channel": "github"
  };

  if (dependencies.storage.getItem(ACTIVATED_KEY) !== "1") {
    const response = await client.signal("App.installActivated", payload);
    if (response.ok) {
      dependencies.storage.setItem(ACTIVATED_KEY, "1");
      result.activationSent = true;
    }
  }

  const todayUtc = new Date(dependencies.now()).toISOString().slice(0, 10);
  if (dependencies.storage.getItem(LAST_ACTIVE_DAY_KEY) !== todayUtc) {
    const response = await client.signal("App.activeDaily", payload);
    if (response.ok) {
      dependencies.storage.setItem(LAST_ACTIVE_DAY_KEY, todayUtc);
      result.dailyActiveSent = true;
    }
  }

  return result;
}

/**
 * Records only an activated installation and a once-per-UTC-day active marker.
 * Call this after a successful final dictation. Nothing is sent before explicit
 * consent, and no transcript, audio, account, filename, or device data enters
 * either event payload.
 */
export function trackAnonymousUsage(
  optedIn: boolean,
  overrides: Partial<AnonymousUsageDependencies> = {}
): Promise<AnonymousUsageResult> {
  const dependencies = { ...defaultDependencies(), ...overrides };
  const next = sendQueue.then(
    () => sendAnonymousUsage(optedIn, dependencies),
    () => sendAnonymousUsage(optedIn, dependencies)
  );
  sendQueue = next;
  return next;
}

export function resetAnonymousUsageForTests(): void {
  activeClient = null;
  sendQueue = Promise.resolve({
    configured: false,
    activationSent: false,
    dailyActiveSent: false
  });
}
