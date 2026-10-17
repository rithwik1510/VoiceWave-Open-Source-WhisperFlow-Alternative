import TelemetryDeck from "@telemetrydeck/sdk";
import { getVersion as getAppVersion } from "@tauri-apps/api/app";

const INSTALLATION_ID_KEY = "voicewave.analytics.installation-id";
const ACTIVATED_KEY = "voicewave.analytics.activated";
const LAST_ACTIVE_DAY_KEY = "voicewave.analytics.last-active-day";
/** Prefix for the funnel "already sent" markers, one key per dedupe slot. */
const FUNNEL_KEY_PREFIX = "voicewave.analytics.funnel.";
/** Mirrors `ANALYTICS_CONSENT_VERSION` in src-tauri/src/settings/mod.rs: the
 * analytics disclosure revision every user must have seen once. */
export const ANALYTICS_CONSENT_VERSION = 1;
/** The in-app answer to "what does the anonymous usage count send?". Keep it
 * in step with the events below whenever one is added or changed. */
export const ANONYMOUS_USAGE_DETAILS =
  "A random install ID, the app version, a first-use marker and a once-a-day active marker, plus setup " +
  "progress, whether your first dictation ran on the graphics card or the processor, model download " +
  "failures, and graphics-card-to-processor fallbacks. Never audio, transcripts, dictionary or snippet " +
  "content, account identity, filenames, or settings.";
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

/**
 * Setup and reliability funnel. Each event carries exactly one enum value on
 * top of the base payload (app version + channel) — never transcript text,
 * error messages, paths, or device names.
 */
export type FunnelEvent =
  | { type: "Onboarding.stepReached"; step: "welcome" | "mic" | "speak" | "done" }
  | { type: "Onboarding.completed"; outcome: "finished" | "skipped" }
  | { type: "Dictation.firstSuccess"; engine: "gpu" | "cpu" }
  | { type: "Model.downloadFailed"; model: "speech-gpu" | "speech-cpu" | "polish" }
  | {
      type: "Speech.gpuStepDown";
      reason: "preparing" | "cooldown" | "driver-disabled" | "unavailable" | "fault";
    };

export type SpeechModelKind = "speech-gpu" | "speech-cpu";

let activeClient: { installationId: string; client: SignalClient } | null = null;
// Every send runs through one queue so a check-then-mark dedupe can never
// race a second copy of the same event (e.g. a model failure reported by both
// the event stream and the failed command).
let sendQueue: Promise<unknown> = Promise.resolve();
/** "Once per app session" markers: this webview lives as long as the app. */
const sessionFunnelKeys = new Set<string>();
/** Whether this install had already been activated before this launch, read
 * once before the first send can set the marker. Installs that dictated on an
 * earlier version must not report a "first" success after updating. */
let activatedBeforeLaunch: boolean | null = null;

function snapshotActivation(storage: Pick<Storage, "getItem">): boolean {
  activatedBeforeLaunch ??= storage.getItem(ACTIVATED_KEY) === "1";
  return activatedBeforeLaunch;
}

function enqueue<T>(task: () => Promise<T>): Promise<T> {
  const next = sendQueue.then(task, task);
  sendQueue = next;
  return next;
}

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

/** The client plus the payload every signal shares (app version + channel). */
async function prepareSignal(dependencies: AnonymousUsageDependencies) {
  const installationId = getOrCreateInstallationId(dependencies);
  const client = dependencies.createClient(dependencies.appId, installationId);
  const appVersion = await dependencies.getAppVersion().catch(() => "unknown");
  const payload: Record<string, string> = {
    "VoiceWave.App.version": appVersion,
    "VoiceWave.Distribution.channel": "github"
  };
  return { client, payload };
}

function utcDay(dependencies: AnonymousUsageDependencies): string {
  return new Date(dependencies.now()).toISOString().slice(0, 10);
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

  snapshotActivation(dependencies.storage);
  const { client, payload } = await prepareSignal(dependencies);

  if (dependencies.storage.getItem(ACTIVATED_KEY) !== "1") {
    const response = await client.signal("App.installActivated", payload);
    if (response.ok) {
      dependencies.storage.setItem(ACTIVATED_KEY, "1");
      result.activationSent = true;
    }
  }

  const todayUtc = utcDay(dependencies);
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
 * Call this after a successful final dictation. Nothing is sent while the user
 * has it turned off, and no transcript, audio, account, filename, or device
 * data enters either event payload.
 */
export function trackAnonymousUsage(
  optedIn: boolean,
  overrides: Partial<AnonymousUsageDependencies> = {}
): Promise<AnonymousUsageResult> {
  const dependencies = { ...defaultDependencies(), ...overrides };
  return enqueue(() => sendAnonymousUsage(optedIn, dependencies));
}

/** The single parameter an event carries, and how often it may be sent:
 * once per install, once per UTC day, or once per app session. `slot` names
 * the dedupe bucket (e.g. one per onboarding step, one for completion). */
function describeFunnelEvent(event: FunnelEvent): {
  parameter: string;
  value: string;
  once: "install" | "utc-day" | "session";
  slot: string;
} {
  switch (event.type) {
    case "Onboarding.stepReached":
      return {
        parameter: "VoiceWave.Onboarding.step",
        value: event.step,
        once: "install",
        slot: `${event.type}.${event.step}`
      };
    case "Onboarding.completed":
      return { parameter: "VoiceWave.Onboarding.outcome", value: event.outcome, once: "install", slot: event.type };
    case "Dictation.firstSuccess":
      return { parameter: "VoiceWave.Dictation.engine", value: event.engine, once: "install", slot: event.type };
    case "Model.downloadFailed":
      return {
        parameter: "VoiceWave.Model.kind",
        value: event.model,
        once: "utc-day",
        slot: `${event.type}.${event.model}`
      };
    case "Speech.gpuStepDown":
      return {
        parameter: "VoiceWave.Speech.stepDownReason",
        value: event.reason,
        once: "session",
        slot: `${event.type}.${event.reason}`
      };
  }
}

async function sendFunnelEvent(
  optedIn: boolean,
  event: FunnelEvent,
  dependencies: AnonymousUsageDependencies
): Promise<boolean> {
  const storage = dependencies.storage;
  if (!optedIn || !dependencies.appId || !storage) {
    return false;
  }

  const { parameter, value, once, slot } = describeFunnelEvent(event);
  const storageKey = `${FUNNEL_KEY_PREFIX}${slot}`;
  if (event.type === "Dictation.firstSuccess" && snapshotActivation(storage)) {
    storage.setItem(storageKey, "1");
    return false;
  }
  const marker = once === "utc-day" ? utcDay(dependencies) : "1";
  const alreadySent =
    once === "session" ? sessionFunnelKeys.has(slot) : storage.getItem(storageKey) === marker;
  if (alreadySent) {
    return false;
  }

  const { client, payload } = await prepareSignal(dependencies);
  const response = await client.signal(event.type, { ...payload, [parameter]: value });
  if (!response.ok) {
    return false;
  }
  if (once === "session") {
    sessionFunnelKeys.add(slot);
  } else {
    storage.setItem(storageKey, marker);
  }
  return true;
}

/**
 * Sends one funnel event, deduplicated per its rule (see `describeFunnelEvent`).
 * Same gate as `trackAnonymousUsage`: nothing leaves the machine unless the
 * user has usage counting on and an app ID is configured. Resolves true when
 * the event was sent.
 */
export function trackFunnelEvent(
  optedIn: boolean,
  event: FunnelEvent,
  overrides: Partial<AnonymousUsageDependencies> = {}
): Promise<boolean> {
  const dependencies = { ...defaultDependencies(), ...overrides };
  return enqueue(() => sendFunnelEvent(optedIn, event, dependencies));
}

/** Speech model family from its id: whisper.cpp ("wcpp-") models are the GPU
 * path, faster-whisper ("fw-") the CPU one. Null for anything else. */
export function speechModelKind(modelId: string): SpeechModelKind | null {
  if (modelId.startsWith("wcpp-")) {
    return "speech-gpu";
  }
  if (modelId.startsWith("fw-")) {
    return "speech-cpu";
  }
  return null;
}

/**
 * Buckets the backend's `asrStepDownReason` (prefixes from
 * src-tauri/src/inference/asr_worker.rs `primary_rung_blocking`). A GPU fault
 * that the CPU retried carries free-form error text, so `backendFallback` is
 * what identifies it. "gpu-retry-recovered" means a fresh GPU worker finished
 * the dictation — not a step-down — and yields null.
 */
export function classifyGpuStepDown(
  reason: string,
  backendFallback: boolean
): Extract<FunnelEvent, { type: "Speech.gpuStepDown" }>["reason"] | null {
  if (reason.startsWith("gpu-retry-recovered")) {
    return null;
  }
  if (reason.startsWith("gpu-cooldown")) {
    return "cooldown";
  }
  if (reason.startsWith("driver-disabled")) {
    return "driver-disabled";
  }
  if (reason.startsWith("gpu-preparing")) {
    return "preparing";
  }
  return backendFallback ? "fault" : "unavailable";
}

export function resetAnonymousUsageForTests(): void {
  activeClient = null;
  sendQueue = Promise.resolve();
  sessionFunnelKeys.clear();
  activatedBeforeLaunch = null;
}
