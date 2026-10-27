import TelemetryDeck from "@telemetrydeck/sdk";
import { getVersion as getAppVersion } from "@tauri-apps/api/app";
import type {
  DictationFailureKind,
  DownloadFailureStage,
  PanicArea,
  TelemetryEvent,
  WorkerExit,
  WorkerKind
} from "../types/voicewave";

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
  "A random install ID, a random ID that changes every launch, the app version, a first-use marker and a " +
  "once-a-day active marker, a once-a-day app-launch marker and whether the app started with Windows, setup " +
  "progress, whether your first dictation ran on the graphics card or the processor, model download failures " +
  "(with a failure category and how far the download got), graphics-card-to-processor fallbacks, the type of " +
  "a failed dictation (for example no microphone or a timeout), the type of a speech helper crash, and which part of the app hit an internal error. Each comes " +
  "with whether speech runs on the graphics card or the processor, the speech model family, the graphics card " +
  "maker, and Windows 10 or 11. Never audio, transcripts, error messages, dictionary or snippet content, " +
  "account identity, device names, filenames, or your other settings.";
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
  /** Whether usage counting is on. Read when a queued send starts and again
   * right before each signal, so turning it off stops sends already queued. */
  isOptedIn: () => boolean;
  /** Context keys added to every signal, read at send time. */
  getContext: () => Record<string, string>;
};

export type AnonymousUsageResult = {
  configured: boolean;
  activationSent: boolean;
  dailyActiveSent: boolean;
};

/**
 * Setup and reliability funnel. Each event carries its own enum parameters on
 * top of the base payload (app version, channel, and the registered context)
 * — never transcript text, error messages, paths, or device names.
 */
export type FunnelEvent =
  | { type: "Onboarding.stepReached"; step: "welcome" | "mic" | "speak" | "done" }
  | { type: "Onboarding.completed"; outcome: "finished" | "skipped" }
  | { type: "Dictation.firstSuccess"; engine: "gpu" | "cpu" }
  | {
      type: "Model.downloadFailed";
      model: SpeechModelKind | "polish";
      /** Known for whisper.cpp/Parakeet downloads only. */
      failureStage?: DownloadFailureStage;
      progressBucket?: DownloadProgressBucket;
    }
  | {
      type: "Speech.gpuStepDown";
      reason: "preparing" | "cooldown" | "driver-disabled" | "unavailable" | "fault";
    }
  | { type: "App.launchedDaily"; launchMode: "autostart" | "manual" }
  | { type: "Dictation.failed"; failureKind: DictationFailureKind }
  | { type: "Speech.workerCrashed"; worker: WorkerKind; exit: WorkerExit }
  | { type: "App.panicked"; area: PanicArea };

/** Speech engine family. Not GPU vs CPU: Parakeet and whisper.cpp run on
 * either, and the `VoiceWave.Speech.path` context already says which. */
export type SpeechModelKind = "parakeet" | "whisper-cpp" | "faster-whisper";

/** How far a failed download got, coarse enough to carry no file sizes. */
export type DownloadProgressBucket = "0" | "1-24" | "25-49" | "50-74" | "75-99" | "100";

let activeClient: { installationId: string; client: SignalClient } | null = null;
/** Set by `useVoiceWave` via `setTelemetryProviders`. Until then nothing is
 * sent and no extra keys are added. */
let consentProvider: () => boolean = () => false;
let contextProvider: () => Record<string, string> = () => ({});
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
    },
    isOptedIn: () => consentProvider(),
    getContext: () => contextProvider()
  };
}

/** Registers the usage-counting consent and the context (speech path, model
 * family, GPU vendor, Windows major) merged into every signal. Both are read
 * at send time, so they should read live state. */
export function setTelemetryProviders(providers: {
  consent: () => boolean;
  context: () => Record<string, string>;
}): void {
  consentProvider = providers.consent;
  contextProvider = providers.context;
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

/** The client plus the payload every signal shares (context, app version,
 * channel). */
async function prepareSignal(dependencies: AnonymousUsageDependencies) {
  const installationId = getOrCreateInstallationId(dependencies);
  const client = dependencies.createClient(dependencies.appId, installationId);
  const appVersion = await dependencies.getAppVersion().catch(() => "unknown");
  const payload: Record<string, string> = {
    ...dependencies.getContext(),
    "VoiceWave.App.version": appVersion,
    "VoiceWave.Distribution.channel": "github"
  };
  return { client, payload };
}

function utcDay(dependencies: AnonymousUsageDependencies): string {
  return new Date(dependencies.now()).toISOString().slice(0, 10);
}

async function sendAnonymousUsage(dependencies: AnonymousUsageDependencies): Promise<AnonymousUsageResult> {
  const result: AnonymousUsageResult = {
    configured: Boolean(dependencies.appId),
    activationSent: false,
    dailyActiveSent: false
  };

  if (!dependencies.isOptedIn() || !dependencies.appId || !dependencies.storage) {
    return result;
  }

  snapshotActivation(dependencies.storage);
  const { client, payload } = await prepareSignal(dependencies);

  if (dependencies.isOptedIn() && dependencies.storage.getItem(ACTIVATED_KEY) !== "1") {
    const response = await client.signal("App.installActivated", payload);
    if (response.ok) {
      dependencies.storage.setItem(ACTIVATED_KEY, "1");
      result.activationSent = true;
    }
  }

  const todayUtc = utcDay(dependencies);
  if (dependencies.isOptedIn() && dependencies.storage.getItem(LAST_ACTIVE_DAY_KEY) !== todayUtc) {
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
 * Call this after a dictation's text was inserted. Nothing is sent while the
 * user has it turned off, and no transcript, audio, account, filename, or
 * device data enters either event payload.
 */
export function trackAnonymousUsage(
  overrides: Partial<AnonymousUsageDependencies> = {}
): Promise<AnonymousUsageResult> {
  const dependencies = { ...defaultDependencies(), ...overrides };
  return enqueue(() => sendAnonymousUsage(dependencies));
}

/** The parameters an event carries, and how often it may be sent: once per
 * install, once per UTC day, or once per app session. `slot` names the dedupe
 * bucket (e.g. one per onboarding step, one per failure kind). */
function describeFunnelEvent(event: FunnelEvent): {
  params: Record<string, string>;
  once: "install" | "utc-day" | "session";
  slot: string;
} {
  switch (event.type) {
    case "Onboarding.stepReached":
      return {
        params: { "VoiceWave.Onboarding.step": event.step },
        once: "install",
        slot: `${event.type}.${event.step}`
      };
    case "Onboarding.completed":
      return { params: { "VoiceWave.Onboarding.outcome": event.outcome }, once: "install", slot: event.type };
    case "Dictation.firstSuccess":
      return { params: { "VoiceWave.Dictation.engine": event.engine }, once: "install", slot: event.type };
    case "Model.downloadFailed":
      // One per stage per day, so a DNS failure and a later disk-full both
      // arrive; events without a stage keep their old slot.
      return {
        params: {
          "VoiceWave.Model.kind": event.model,
          ...(event.failureStage && { "VoiceWave.Model.failureStage": event.failureStage }),
          ...(event.progressBucket && { "VoiceWave.Model.progressBucket": event.progressBucket })
        },
        once: "utc-day",
        slot: event.failureStage
          ? `${event.type}.${event.model}.${event.failureStage}`
          : `${event.type}.${event.model}`
      };
    case "Speech.gpuStepDown":
      return {
        params: { "VoiceWave.Speech.stepDownReason": event.reason },
        once: "session",
        slot: `${event.type}.${event.reason}`
      };
    case "App.launchedDaily":
      return { params: { "VoiceWave.App.launchMode": event.launchMode }, once: "utc-day", slot: event.type };
    case "Dictation.failed":
      return {
        params: { "VoiceWave.Dictation.failureKind": event.failureKind },
        once: "utc-day",
        slot: `${event.type}.${event.failureKind}`
      };
    case "Speech.workerCrashed":
      return {
        params: { "VoiceWave.Worker.kind": event.worker, "VoiceWave.Worker.exit": event.exit },
        once: "session",
        slot: `${event.type}.${event.worker}+${event.exit}`
      };
    case "App.panicked":
      return { params: { "VoiceWave.Panic.area": event.area }, once: "utc-day", slot: `${event.type}.${event.area}` };
  }
}

async function sendFunnelEvent(event: FunnelEvent, dependencies: AnonymousUsageDependencies): Promise<boolean> {
  const storage = dependencies.storage;
  if (!dependencies.isOptedIn() || !dependencies.appId || !storage) {
    return false;
  }

  const { params, once, slot } = describeFunnelEvent(event);
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
  if (!dependencies.isOptedIn()) {
    return false;
  }
  const response = await client.signal(event.type, { ...payload, ...params });
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
  event: FunnelEvent,
  overrides: Partial<AnonymousUsageDependencies> = {}
): Promise<boolean> {
  const dependencies = { ...defaultDependencies(), ...overrides };
  return enqueue(() => sendFunnelEvent(event, dependencies));
}

/** Speech engine family from a model id: Parakeet (a "wcpp-" id, so checked
 * first), other whisper.cpp ("wcpp-") models, or faster-whisper ("fw-"). Null
 * for anything else. */
export function speechModelKind(modelId: string): SpeechModelKind | null {
  if (modelId.includes("parakeet")) {
    return "parakeet";
  }
  if (modelId.startsWith("wcpp-")) {
    return "whisper-cpp";
  }
  if (modelId.startsWith("fw-")) {
    return "faster-whisper";
  }
  return null;
}

export function downloadProgressBucket(progress: number): DownloadProgressBucket {
  if (progress >= 100) {
    return "100";
  }
  if (progress >= 75) {
    return "75-99";
  }
  if (progress >= 50) {
    return "50-74";
  }
  if (progress >= 25) {
    return "25-49";
  }
  return progress >= 1 ? "1-24" : "0";
}

/** The `VoiceWave.Speech.modelFamily` context value for an active model id. */
export function speechModelFamily(modelId: string): "parakeet" | "turbo" | "small" | "other" | "none" {
  if (!modelId) {
    return "none";
  }
  if (modelId.includes("parakeet")) {
    return "parakeet";
  }
  if (modelId.includes("turbo")) {
    return "turbo";
  }
  return modelId.includes("small") ? "small" : "other";
}

/** Maps a fact drained from the backend queue (`drain_telemetry`) to the
 * funnel event that reports it. */
export function funnelEventFromTelemetry(event: TelemetryEvent): FunnelEvent {
  switch (event.type) {
    case "dictation-failed":
      return { type: "Dictation.failed", failureKind: event.kind };
    case "worker-crashed":
      return { type: "Speech.workerCrashed", worker: event.worker, exit: event.exit };
    case "panicked":
      return { type: "App.panicked", area: event.area };
  }
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
  consentProvider = () => false;
  contextProvider = () => ({});
  sendQueue = Promise.resolve();
  sessionFunnelKeys.clear();
  activatedBeforeLaunch = null;
}
