import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  addDictionaryTerm as addDictionaryTermCommand,
  addVoiceSnippet as addVoiceSnippetCommand,
  approveDictionaryEntry,
  cancelModelDownload,
  canUseTauri,
  clearHistory,
  downloadModel,
  exportDiagnosticsBundle as exportDiagnosticsBundleCommand,
  exportDictionary as exportDictionaryCommand,
  exportSessionHistoryPreset,
  importDictionary as importDictionaryCommand,
  getBenchmarkResults,
  getDiagnosticsStatus,
  getDictionaryQueue,
  getDictionaryTerms,
  getEntitlementSnapshot,
  getHistoryRetention,
  getPermissionSnapshot,
  getRecentInsertions,
  getSessionHistory,
  insertText,
  listenVoicewaveAudioQuality,
  listenVoicewaveHistoryUpdated,
  listenVoicewaveHotkey,
  listenVoicewaveInsertion,
  listenVoicewaveLatency,
  listenVoicewaveModel,
  listenVoicewavePermission,
  listenVoicewaveState,
  listenVoicewaveTranscript,
  listInstalledModels,
  listInputDevices,
  listModelCatalog,
  listVoiceSnippets,
  loadHotkeyConfig,
  loadSnapshot,
  loadSettings,
  openBillingPortal,
  pauseModelDownload,
  pruneHistoryNow,
  refreshEntitlement as refreshEntitlementCommand,
  recommendModel,
  rejectDictionaryEntry,
  removeDictionaryTerm,
  removeVoiceSnippet as removeVoiceSnippetCommand,
  requestMicrophoneAccess,
  restorePurchase as restorePurchaseCommand,
  resumeModelDownload,
  searchSessionHistory as searchSessionHistoryCommand,
  setActiveDomainPacks as setActiveDomainPacksCommand,
  setDiagnosticsOptIn as setDiagnosticsOptInCommand,
  setAppProfileOverrides as setAppProfileOverridesCommand,
  setCodeModeSettings as setCodeModeSettingsCommand,
  setDictationProfile as setDictationProfileCommand,
  setFormatProfile as setFormatProfileCommand,
  setProPostProcessingEnabled as setProPostProcessingEnabledCommand,
  runAudioQualityDiagnostic as runAudioQualityDiagnosticCommand,
  runModelBenchmark,
  setActiveModel,
  setHistoryRetention,
  setOwnerDeviceOverride as setOwnerDeviceOverrideCommand,
  startMicLevelMonitor,
  startProCheckout as startProCheckoutCommand,
  stopMicLevelMonitor,
  stopDictation as stopDictationCommand,
  startDictation,
  tagSession as tagSessionCommand,
  toggleStarSession as toggleStarSessionCommand,
  triggerHotkeyAction,
  undoLastInsertion,
  updateHotkeyConfig,
  updateVoiceSnippet as updateVoiceSnippetCommand,
  updateSettings,
  downloadPolishModel,
  getPolishModelStatus,
  listenVoicewavePolishModelProgress,
  type PolishModelProgress
} from "../lib/tauri";
import { syncDictionaryWithCloud as syncDictionaryRecordsWithCloud } from "../lib/dictionarySync";
import { syncVoiceSnippetsWithCloud as syncVoiceSnippetRecordsWithCloud } from "../lib/snippetSync";
import { trackAnonymousUsage } from "../lib/telemetry";
import type {
  AppProfileOverrides,
  CodeModeSettings,
  DomainPackId,
  EntitlementSnapshot,
  AudioQualityReport,
  BenchmarkRun,
  DecodeMode,
  DiagnosticsExportResult,
  DiagnosticsStatus,
  DictationControlMode,
  DictationMode,
  DictionaryExport,
  DictionaryImportSummary,
  DictionaryQueueItem,
  DictionaryTerm,
  HotkeyConfig,
  HotkeyEvent,
  HotkeySnapshot,
  InsertResult,
  InstalledModel,
  ModelCatalogItem,
  ModelRecommendation,
  ModelEvent,
  ModelStatus,
  MicVolumeGuardMode,
  PermissionSnapshot,
  PolishProfile,
  ProFeatureId,
  RecentInsertion,
  RetentionPolicy,
  ThemePreference,
  FormatProfile,
  HistoryExportPreset,
  HistoryExportResult,
  SessionHistoryRecord,
  LatencyBreakdownEvent,
  VoiceWaveHudState,
  VoiceSnippet,
  VoiceWaveSettings,
  VoiceWaveSnapshot
} from "../types/voicewave";

const DEFAULT_MAX_UTTERANCE_MS = 120_000;
const MIN_MAX_UTTERANCE_MS = 5_000;
const MAX_MAX_UTTERANCE_MS = 180_000;
const DEFAULT_RELEASE_TAIL_MS = 350;
const MIN_RELEASE_TAIL_MS = 120;
const MAX_RELEASE_TAIL_MS = 1_500;
const DEFAULT_DECODE_MODE: DecodeMode = "balanced";
const SUPPORTED_MODEL_IDS = ["fw-small.en", "fw-large-v3-turbo"] as const;
// Retired catalog models migrate to the closest surviving option (mirrors
// normalize_active_model_id in src-tauri/src/settings/mod.rs).
const RETIRED_LARGE_MODEL_IDS = ["fw-large-v3", "wcpp-large-v3-turbo"] as const;
const LEGACY_MODEL_IDS = ["tiny.en", "base.en", "small.en", "medium.en", "wcpp-small.en"] as const;

function isSupportedModelId(modelId: string): boolean {
  return SUPPORTED_MODEL_IDS.includes(modelId as (typeof SUPPORTED_MODEL_IDS)[number]);
}

/** faster-whisper downloads run inside the bundled python runtime, which has no
 * pause/resume: they can only be cancelled and restarted (HuggingFace keeps the
 * partial blobs, so a restart range-resumes). Catalog format is
 * "faster-whisper"; ids are prefixed "fw-" (mirrors is_faster_whisper_model). */
function isFasterWhisperModelId(modelId: string): boolean {
  return modelId.startsWith("fw-");
}

const fallbackSettings: VoiceWaveSettings = {
  inputDevice: null,
  activeModel: "fw-small.en",
  showFloatingHud: true,
  vadThreshold: 0.014,
  maxUtteranceMs: DEFAULT_MAX_UTTERANCE_MS,
  releaseTailMs: DEFAULT_RELEASE_TAIL_MS,
  decodeMode: DEFAULT_DECODE_MODE,
  diagnosticsOptIn: false,
  anonymousUsageOptIn: false,
  toggleHotkey: "Ctrl+Alt+X",
  pushToTalkHotkey: "Ctrl+Windows",
  preferClipboardFallback: false,
  formatProfile: "default",
  activeDomainPacks: [],
  appProfileOverrides: {
    activeTarget: "editor",
    editor: { punctuationAggressiveness: 2, sentenceCompactness: 1, autoListFormatting: true },
    browser: { punctuationAggressiveness: 1, sentenceCompactness: 1, autoListFormatting: false },
    collab: { punctuationAggressiveness: 1, sentenceCompactness: 2, autoListFormatting: true },
    desktop: { punctuationAggressiveness: 1, sentenceCompactness: 1, autoListFormatting: false }
  },
  codeMode: {
    enabled: false,
    spokenSymbols: true,
    preferredCasing: "preserve",
    wrapInFencedBlock: false
  },
  proPostProcessingEnabled: false,
  spokenEditCommands: true,
  micVolumeGuard: "warn",
  // True in the pre-load fallback so the onboarding flow never flashes before
  // real settings arrive; the backend value replaces this immediately.
  onboardingCompleted: true,
  theme: "system",
  polishProfile: "standard",
  polishProfileCustomized: false
};

const fallbackSnapshot: VoiceWaveSnapshot = {
  state: "idle",
  lastPartial: null,
  lastFinal: null,
  activeModel: "fw-small.en"
};

const fallbackHotkeys: HotkeySnapshot = {
  config: {
    toggle: "Ctrl+Alt+X",
    pushToTalk: "Ctrl+Windows"
  },
  conflicts: [],
  registrationSupported: true,
  registrationError: null
};

const fallbackPermissions: PermissionSnapshot = {
  microphone: "unknown",
  insertionCapability: "available",
  message: "Permissions are managed in desktop runtime."
};

const fallbackDiagnosticsStatus: DiagnosticsStatus = {
  optIn: false,
  recordCount: 0,
  lastExportPath: null,
  lastExportedAtUtcMs: null,
  watchdogRecoveryCount: 0
};

const fallbackEntitlement: EntitlementSnapshot = {
  tier: "pro",
  status: "pro_active",
  isPro: true,
  isOwnerOverride: false,
  expiresAtUtcMs: null,
  graceUntilUtcMs: null,
  lastRefreshedAtUtcMs: 0,
  plan: {
    basePriceUsdMonthly: 0,
    launchPriceUsdMonthly: 0,
    launchMonths: 0,
    displayBasePrice: "Included",
    displayLaunchPrice: "Included",
    offerCopy: "Initial release offer: Pro is included for everyone."
  },
  message: null
};

const fallbackModelCatalog: ModelCatalogItem[] = [
  {
    modelId: "fw-small.en",
    displayName: "faster-whisper small.en",
    version: "faster-whisper-v1",
    format: "faster-whisper",
    sizeBytes: 487_614_201,
    sha256: "000000000000000000000000000000000000000000000000000000001d10d5f9",
    license: "MIT (faster-whisper + model license)",
    downloadUrl: "faster-whisper://small.en",
    signature: "local"
  },
  {
    modelId: "fw-large-v3-turbo",
    displayName: "faster-whisper large-v3-turbo (~1.6 GB, best accuracy on GPU)",
    version: "faster-whisper-v1",
    format: "faster-whisper",
    sizeBytes: 1_620_000_000,
    sha256: "0000000000000000000000000000000000000000000000000000000060907f00",
    license: "MIT (faster-whisper + model license)",
    downloadUrl: "faster-whisper://large-v3-turbo",
    signature: "local"
  }
];

const RECOMMENDED_VAD_THRESHOLD = 0.014;
const MIN_VAD_THRESHOLD = 0.005;
const MAX_VAD_THRESHOLD = 0.04;
const LOCKED_TOGGLE_HOTKEY = "Ctrl+Alt+X";
const LOCKED_PUSH_TO_TALK_HOTKEY = "Ctrl+Windows";
const MODIFIER_TOKENS = [
  "CTRL",
  "CONTROL",
  "SHIFT",
  "ALT",
  "OPTION",
  "META",
  "SUPER",
  "CMD",
  "WIN",
  "WINDOWS"
];
export interface MicQualityWarning {
  currentDevice: string;
  message: string;
  recommendedDevice: string | null;
}

function splitCombo(combo: string): string[] {
  return combo
    .split("+")
    .map((part) => part.trim().toUpperCase())
    .filter(Boolean);
}

function isModifierToken(token: string): boolean {
  return MODIFIER_TOKENS.includes(token);
}

function getComboMainKey(combo: string): string | null {
  const tokens = splitCombo(combo);
  const main = tokens.find((token) => !isModifierToken(token));
  return main ?? null;
}

function comboIsModifierOnly(combo: string): boolean {
  const tokens = splitCombo(combo);
  return tokens.length > 0 && tokens.every((token) => isModifierToken(token));
}

function eventMatchesMainKey(event: KeyboardEvent, mainKey: string): boolean {
  if (mainKey === "SPACE") {
    return event.code === "Space" || event.key === " ";
  }
  return event.key.toUpperCase() === mainKey;
}

function isEditableTarget(target: EventTarget | null): boolean {
  const element = target as HTMLElement | null;
  if (!element) {
    return false;
  }
  return element.tagName === "INPUT" || element.tagName === "TEXTAREA" || element.isContentEditable;
}

function comboMatchesKeyboardEvent(event: KeyboardEvent, combo: string): boolean {
  const tokens = splitCombo(combo);
  if (tokens.length === 0) {
    return false;
  }

  const expectsCtrl = tokens.includes("CTRL") || tokens.includes("CONTROL");
  const expectsAlt = tokens.includes("ALT") || tokens.includes("OPTION");
  const expectsShift = tokens.includes("SHIFT");
  const expectsMeta =
    tokens.includes("META") ||
    tokens.includes("SUPER") ||
    tokens.includes("CMD") ||
    tokens.includes("WIN") ||
    tokens.includes("WINDOWS");

  if (event.ctrlKey !== expectsCtrl) {
    return false;
  }
  if (event.altKey !== expectsAlt) {
    return false;
  }
  if (event.shiftKey !== expectsShift) {
    return false;
  }
  if (event.metaKey !== expectsMeta) {
    return false;
  }

  const main = tokens.find((token) => !isModifierToken(token));
  if (!main) {
    const key = event.key.toUpperCase();
    const modifierKeyMatch =
      (expectsCtrl && (key === "CONTROL" || key === "CTRL")) ||
      (expectsShift && key === "SHIFT") ||
      (expectsAlt && (key === "ALT" || key === "OPTION")) ||
      (expectsMeta && (key === "META" || key === "OS" || key === "WIN" || key === "WINDOWS"));
    return modifierKeyMatch;
  }

  if (main === "SPACE") {
    return event.code === "Space" || event.key === " ";
  }

  return event.key.toUpperCase() === main;
}

function clampVadThreshold(value: number): number {
  if (!Number.isFinite(value)) {
    return RECOMMENDED_VAD_THRESHOLD;
  }
  return Math.min(MAX_VAD_THRESHOLD, Math.max(MIN_VAD_THRESHOLD, value));
}

function clampMaxUtteranceMs(value: number): number {
  if (!Number.isFinite(value)) {
    return DEFAULT_MAX_UTTERANCE_MS;
  }
  return Math.round(Math.min(MAX_MAX_UTTERANCE_MS, Math.max(MIN_MAX_UTTERANCE_MS, value)));
}

function clampReleaseTailMs(value: number): number {
  if (!Number.isFinite(value)) {
    return DEFAULT_RELEASE_TAIL_MS;
  }
  return Math.round(Math.min(MAX_RELEASE_TAIL_MS, Math.max(MIN_RELEASE_TAIL_MS, value)));
}

function normalizeActiveModel(activeModel: string): string {
  if (SUPPORTED_MODEL_IDS.includes(activeModel as (typeof SUPPORTED_MODEL_IDS)[number])) {
    return activeModel;
  }
  if (RETIRED_LARGE_MODEL_IDS.includes(activeModel as (typeof RETIRED_LARGE_MODEL_IDS)[number])) {
    return "fw-large-v3-turbo";
  }
  if (LEGACY_MODEL_IDS.includes(activeModel as (typeof LEGACY_MODEL_IDS)[number])) {
    return "fw-small.en";
  }
  return "fw-small.en";
}

function normalizeSettings(settings: VoiceWaveSettings): VoiceWaveSettings {
  return {
    ...settings,
    activeModel: normalizeActiveModel(settings.activeModel),
    vadThreshold: clampVadThreshold(settings.vadThreshold),
    maxUtteranceMs: clampMaxUtteranceMs(settings.maxUtteranceMs ?? DEFAULT_MAX_UTTERANCE_MS),
    releaseTailMs: clampReleaseTailMs(settings.releaseTailMs ?? DEFAULT_RELEASE_TAIL_MS),
    decodeMode: settings.decodeMode ?? DEFAULT_DECODE_MODE,
    diagnosticsOptIn: settings.diagnosticsOptIn ?? false,
    anonymousUsageOptIn: settings.anonymousUsageOptIn ?? false,
    formatProfile: settings.formatProfile ?? "default",
    activeDomainPacks: settings.activeDomainPacks ?? [],
    appProfileOverrides: settings.appProfileOverrides ?? fallbackSettings.appProfileOverrides,
    codeMode: settings.codeMode ?? fallbackSettings.codeMode,
    proPostProcessingEnabled: settings.proPostProcessingEnabled ?? false,
    spokenEditCommands: settings.spokenEditCommands ?? true,
    micVolumeGuard: settings.micVolumeGuard ?? "warn",
    onboardingCompleted: settings.onboardingCompleted ?? false,
    theme: settings.theme ?? "system",
    // polishProfile / polishProfileCustomized are deliberately NOT defaulted:
    // their absence is the "legacy backend" signal the UI uses to fall back
    // to inferring a profile from the old deterministic fields (plan 010).
    toggleHotkey: LOCKED_TOGGLE_HOTKEY,
    pushToTalkHotkey: LOCKED_PUSH_TO_TALK_HOTKEY
  };
}

function normalizeDeviceLabel(value: string): string {
  return value.trim().toLowerCase();
}

function isLikelyLowQualityMic(deviceName: string): boolean {
  const normalized = normalizeDeviceLabel(deviceName);
  return (
    normalized.includes("hands-free") ||
    normalized.includes("hand free") ||
    normalized.includes("bluetooth headset") ||
    normalized.includes("headset") ||
    normalized.includes("hfp") ||
    normalized.includes("ag audio") ||
    normalized.includes("sco")
  );
}

function findRecommendedInputDevice(
  deviceNames: string[],
  currentDevice: string | null
): string | null {
  const current = currentDevice ? normalizeDeviceLabel(currentDevice) : null;
  const candidates = deviceNames.filter((name) => normalizeDeviceLabel(name) !== current);
  const preferred = candidates.find((name) => !isLikelyLowQualityMic(name));
  return preferred ?? null;
}

function deriveModelStatuses(
  catalog: ModelCatalogItem[],
  installed: InstalledModel[],
  activeModel: string,
  previous: Record<string, ModelStatus>
): Record<string, ModelStatus> {
  const installedMap = new Map(installed.map((row) => [row.modelId, row]));
  const next: Record<string, ModelStatus> = {};
  for (const item of catalog) {
    const prior = previous[item.modelId];
    if (prior && (prior.state === "downloading" || prior.state === "failed" || prior.state === "cancelled")) {
      next[item.modelId] = { ...prior, active: item.modelId === activeModel };
      continue;
    }
    const installedModel = installedMap.get(item.modelId) ?? null;
      next[item.modelId] = {
        modelId: item.modelId,
        state: installedModel ? "installed" : "idle",
        progress: installedModel ? 100 : 0,
        active: item.modelId === activeModel,
        installed: Boolean(installedModel),
        message: installedModel ? "Installed and checksum verified." : "Not installed.",
        installedModel,
        downloadedBytes: installedModel ? installedModel.sizeBytes : 0,
        totalBytes: item.sizeBytes,
        resumable: false
      };
  }
  return next;
}

function parseProRequiredFeature(error: unknown): ProFeatureId | null {
  const message = error instanceof Error ? error.message : String(error ?? "");
  const marker = "PRO_REQUIRED:";
  const idx = message.indexOf(marker);
  if (idx < 0) {
    return null;
  }
  const feature = message.slice(idx + marker.length).trim();
  const allowed: ProFeatureId[] = [
    "format_profile",
    "domain_packs",
    "app_profiles",
    "code_mode",
    "post_processing",
    "advanced_history"
  ];
  return allowed.includes(feature as ProFeatureId) ? (feature as ProFeatureId) : null;
}

export function useVoiceWave() {
  const [snapshot, setSnapshot] = useState<VoiceWaveSnapshot>(fallbackSnapshot);
  const [settings, setSettings] = useState<VoiceWaveSettings>(fallbackSettings);
  const settingsRef = useRef(settings);
  settingsRef.current = settings;
  const [hotkeys, setHotkeys] = useState<HotkeySnapshot>(fallbackHotkeys);
  const [permissions, setPermissions] = useState<PermissionSnapshot>(fallbackPermissions);
  const [inputDevices, setInputDevices] = useState<string[]>([]);
  const [audioQualityReport, setAudioQualityReport] = useState<AudioQualityReport | null>(null);
  const [lastLatency, setLastLatency] = useState<LatencyBreakdownEvent | null>(null);
  const [diagnosticsStatus, setDiagnosticsStatus] =
    useState<DiagnosticsStatus>(fallbackDiagnosticsStatus);
  const [lastDiagnosticsExport, setLastDiagnosticsExport] =
    useState<DiagnosticsExportResult | null>(null);
  const [entitlement, setEntitlement] = useState<EntitlementSnapshot>(fallbackEntitlement);
  const [lastHistoryExport, setLastHistoryExport] = useState<HistoryExportResult | null>(null);
  const [recentInsertions, setRecentInsertions] = useState<RecentInsertion[]>([]);
  const [lastInsertion, setLastInsertion] = useState<InsertResult | null>(null);
  const [lastHotkeyEvent, setLastHotkeyEvent] = useState<HotkeyEvent | null>(null);

  const [modelCatalog, setModelCatalog] = useState<ModelCatalogItem[]>(fallbackModelCatalog);
  const [installedModels, setInstalledModels] = useState<InstalledModel[]>([]);
  const [modelStatuses, setModelStatuses] = useState<Record<string, ModelStatus>>(
    deriveModelStatuses(
      fallbackModelCatalog,
      [],
      fallbackSettings.activeModel,
      {}
    )
  );
  const [benchmarkResults, setBenchmarkResults] = useState<BenchmarkRun | null>(null);
  const [modelRecommendation, setModelRecommendation] = useState<ModelRecommendation | null>(null);
  const [modelSpeeds, setModelSpeeds] = useState<Record<string, number>>({});
  const speedSamples = useRef<Record<string, { bytes: number; time: number }>>({});

  const [historyPolicy, setHistoryPolicy] = useState<RetentionPolicy>("days30");
  const [sessionHistory, setSessionHistory] = useState<SessionHistoryRecord[]>([]);
  const [dictionaryQueue, setDictionaryQueue] = useState<DictionaryQueueItem[]>([]);
  const [dictionaryTerms, setDictionaryTerms] = useState<DictionaryTerm[]>([]);
  const [voiceSnippets, setVoiceSnippets] = useState<VoiceSnippet[]>([]);

  const [tauriAvailable] = useState<boolean>(() => canUseTauri());
  const [isBusy, setIsBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const timeoutHandles = useRef<number[]>([]);
  const pushToTalkLatchedRef = useRef(false);

  const clearWebTimers = useCallback(() => {
    timeoutHandles.current.forEach((timeoutId) => {
      window.clearTimeout(timeoutId);
    });
    timeoutHandles.current = [];
  }, []);

  const refreshRecentInsertions = useCallback(async () => {
    if (!tauriAvailable) {
      return;
    }
    try {
      setRecentInsertions(await getRecentInsertions(8));
    } catch (refreshErr) {
      setError(refreshErr instanceof Error ? refreshErr.message : "Failed to load recent insertions");
    }
  }, [tauriAvailable]);

  const refreshInputDevices = useCallback(async () => {
    if (!tauriAvailable) {
      return;
    }
    try {
      const devices = await listInputDevices();
      setInputDevices(devices);
    } catch (deviceErr) {
      setError(deviceErr instanceof Error ? deviceErr.message : "Failed to list input devices");
    }
  }, [tauriAvailable]);

  const refreshEntitlement = useCallback(async () => {
    if (!tauriAvailable) {
      setEntitlement(fallbackEntitlement);
      return fallbackEntitlement;
    }
    try {
      const snapshot = await refreshEntitlementCommand();
      setEntitlement(snapshot);
      return snapshot;
    } catch (entitlementErr) {
      setError(entitlementErr instanceof Error ? entitlementErr.message : "Failed to refresh entitlement.");
      return entitlement;
    }
  }, [entitlement, tauriAvailable]);

  const canUseFeature = useCallback(
    (_featureId: ProFeatureId): boolean => {
      return entitlement.isPro;
    },
    [entitlement.isPro]
  );

  const startProCheckoutAction = useCallback(async () => {
    try {
      const launch = tauriAvailable
        ? await startProCheckoutCommand()
        : { url: "", launched: false, message: "Checkout is disabled in web fallback mode." };
      await refreshEntitlement();
      return launch;
    } catch (checkoutErr) {
      setError(checkoutErr instanceof Error ? checkoutErr.message : "Failed to start checkout.");
      throw checkoutErr;
    }
  }, [refreshEntitlement, tauriAvailable]);

  const restoreProPurchase = useCallback(async () => {
    if (!tauriAvailable) {
      return fallbackEntitlement;
    }
    try {
      const snapshot = await restorePurchaseCommand();
      setEntitlement(snapshot);
      return snapshot;
    } catch (restoreErr) {
      setError(restoreErr instanceof Error ? restoreErr.message : "Failed to restore purchase.");
      throw restoreErr;
    }
  }, [tauriAvailable]);

  const openProBillingPortal = useCallback(async () => {
    try {
      const launch = tauriAvailable
        ? await openBillingPortal()
        : { url: "", launched: false, message: "Billing portal is disabled in web fallback mode." };
      return launch;
    } catch (portalErr) {
      setError(portalErr instanceof Error ? portalErr.message : "Failed to open billing portal.");
      throw portalErr;
    }
  }, [tauriAvailable]);

  const setOwnerOverride = useCallback(
    async (enabled: boolean, passphrase: string) => {
      if (!tauriAvailable) {
        const next: EntitlementSnapshot = {
          ...fallbackEntitlement,
          isPro: true,
          isOwnerOverride: true,
          tier: "pro",
          status: "owner_override"
        };
        setEntitlement(next);
        return next;
      }
      try {
        const snapshot = await setOwnerDeviceOverrideCommand(enabled, passphrase);
        setEntitlement(snapshot);
        return snapshot;
      } catch (ownerErr) {
        setError(ownerErr instanceof Error ? ownerErr.message : "Owner override failed.");
        throw ownerErr;
      }
    },
    [tauriAvailable]
  );

  const refreshPhase3Data = useCallback(async (activeModelOverride?: string) => {
    if (!tauriAvailable) {
      return;
    }
    try {
      const [catalogRows, installedRows, historyRows, queueRows, termRows, snippetRows, benchmark] = await Promise.all([
        listModelCatalog(),
        listInstalledModels(),
        getSessionHistory({ includeFailed: true, limit: 50 }),
        getDictionaryQueue(50),
        getDictionaryTerms(),
        listVoiceSnippets(),
        getBenchmarkResults()
      ]);
      const supportedCatalog = catalogRows.filter((row) => isSupportedModelId(row.modelId));
      const supportedInstalled = installedRows.filter((row) => isSupportedModelId(row.modelId));
      setModelCatalog(supportedCatalog);
      setInstalledModels(supportedInstalled);
      setModelStatuses((prev) =>
        deriveModelStatuses(
          supportedCatalog,
          supportedInstalled,
          activeModelOverride ?? settings.activeModel,
          prev
        )
      );
      setSessionHistory(historyRows);
      setDictionaryQueue(queueRows);
      setDictionaryTerms(termRows);
      setVoiceSnippets(snippetRows);
      setBenchmarkResults(benchmark);
      if (benchmark) {
        try {
          setModelRecommendation(await recommendModel());
        } catch {
          setModelRecommendation(null);
        }
      }
    } catch (loadErr) {
      setError(loadErr instanceof Error ? loadErr.message : "Failed to load Phase III data.");
    }
  }, [settings.activeModel, tauriAvailable]);

  const refreshDictionary = useCallback(async () => {
    if (!tauriAvailable) {
      return;
    }
    try {
      const [queueRows, termRows] = await Promise.all([
        getDictionaryQueue(50),
        getDictionaryTerms()
      ]);
      setDictionaryQueue(queueRows);
      setDictionaryTerms(termRows);
    } catch (dictionaryErr) {
      setError(dictionaryErr instanceof Error ? dictionaryErr.message : "Failed to load dictionary.");
      throw dictionaryErr;
    }
  }, [tauriAvailable]);

  const syncDictionaryWithCloud = useCallback(async (uid: string) => {
    if (!tauriAvailable) {
      return [];
    }
    const terms = await syncDictionaryRecordsWithCloud(uid);
    setDictionaryTerms(terms);
    return terms;
  }, [tauriAvailable]);

  const refreshVoiceSnippets = useCallback(async () => {
    if (!tauriAvailable) {
      return;
    }
    try {
      setVoiceSnippets(await listVoiceSnippets());
    } catch (snippetErr) {
      setError(snippetErr instanceof Error ? snippetErr.message : "Failed to load snippets.");
      throw snippetErr;
    }
  }, [tauriAvailable]);

  const syncVoiceSnippetsWithCloud = useCallback(async (
    uid: string,
    isCurrent?: () => boolean
  ) => {
    if (!tauriAvailable) {
      return { snippets: [], records: [], limitExceeded: false };
    }
    try {
      const result = await syncVoiceSnippetRecordsWithCloud(uid, isCurrent);
      setVoiceSnippets(result.snippets);
      return result;
    } catch (syncErr) {
      // Reconciliation commits locally before replication. Keep the UI true
      // to the encrypted local store even when the cloud write fails.
      try {
        setVoiceSnippets(await listVoiceSnippets());
      } catch {
        // Preserve the original sync error; refreshPhase3Data can retry later.
      }
      throw syncErr;
    }
  }, [tauriAvailable]);

  // Lightweight history-only refresh for the post-dictation event; avoids the
  // full refreshPhase3Data reload (model catalog, dictionary, benchmarks).
  const refreshSessionHistory = useCallback(async () => {
    if (!tauriAvailable) {
      return;
    }
    try {
      setSessionHistory(await getSessionHistory({ includeFailed: true, limit: 50 }));
    } catch {
      // Non-critical; the next full refresh will catch up.
    }
  }, [tauriAvailable]);

  const runWebFixtureDemo = useCallback((controlMode: DictationControlMode) => {
    clearWebTimers();
    setError(null);
    setIsBusy(true);
    setSnapshot((prev) => ({ ...prev, state: "listening", lastPartial: null, lastFinal: null, controlMode }));

    timeoutHandles.current.push(
      window.setTimeout(() => {
        setSnapshot((prev) => ({ ...prev, state: "transcribing", lastPartial: "phase three model manager" }));
      }, 700)
    );

    timeoutHandles.current.push(
      window.setTimeout(() => {
        setSnapshot((prev) => ({
          ...prev,
          state: "inserted",
          lastFinal: "phase three controls are wired and ready",
          lastPartial: null
        }));
      }, 1700)
    );

    timeoutHandles.current.push(
      window.setTimeout(() => {
        setSnapshot((prev) => ({ ...prev, state: "idle", controlMode: null }));
        setIsBusy(false);
      }, 2400)
    );
  }, [clearWebTimers]);

  const ensureDictationModelReady = useCallback(async () => {
    const activeInstalled = installedModels.some((row) => row.modelId === settings.activeModel);
    if (activeInstalled) {
      return settings.activeModel;
    }

    if (settings.activeModel.startsWith("fw-")) {
      const fwStatus = await downloadModel({ modelId: settings.activeModel });
      setModelStatuses((prev) => ({ ...prev, [settings.activeModel]: fwStatus }));
      if (fwStatus.state === "installed") {
        await refreshPhase3Data(settings.activeModel);
        return settings.activeModel;
      }
    }

    const preferredInstalledOrder = ["fw-small.en", "fw-large-v3-turbo"];
    const installedSet = new Set(installedModels.map((row) => row.modelId));
    const fallbackInstalled =
      preferredInstalledOrder.find((modelId) => installedSet.has(modelId)) ??
      installedModels[0]?.modelId ??
      null;

    if (fallbackInstalled) {
      const nextSettings = normalizeSettings(await setActiveModel(fallbackInstalled));
      setSettings(nextSettings);
      setSnapshot((prev) => ({ ...prev, activeModel: nextSettings.activeModel }));
      await refreshPhase3Data(nextSettings.activeModel);
      return nextSettings.activeModel;
    }

    const bootstrapModelId =
      modelCatalog.find((row) => row.modelId === "fw-small.en")?.modelId ??
      modelCatalog.find((row) => row.modelId === "fw-large-v3-turbo")?.modelId ??
      modelCatalog[0]?.modelId ??
      null;
    if (!bootstrapModelId) {
      throw new Error("No models are available in catalog. Open Models and refresh runtime state.");
    }

    const status = await downloadModel({ modelId: bootstrapModelId });
    setModelStatuses((prev) => ({ ...prev, [bootstrapModelId]: status }));
    if (status.state !== "installed") {
      throw new Error(status.message ?? `Model install did not complete (${status.state}).`);
    }

    const nextSettings = normalizeSettings(await setActiveModel(bootstrapModelId));
    setSettings(nextSettings);
    setSnapshot((prev) => ({ ...prev, activeModel: nextSettings.activeModel }));
    await refreshPhase3Data(nextSettings.activeModel);
    return nextSettings.activeModel;
  }, [installedModels, modelCatalog, refreshPhase3Data, settings.activeModel]);

  const runDictation = useCallback(
    async (
      mode: DictationMode = "microphone",
      controlMode: DictationControlMode = "holdToTalk"
    ) => {
      if (!tauriAvailable) {
        runWebFixtureDemo(controlMode);
        return;
      }

      try {
        if (mode === "microphone") {
          const permissionSnapshot = await requestMicrophoneAccess();
          setPermissions(permissionSnapshot);
          if (permissionSnapshot.microphone !== "granted") {
            setError(
              permissionSnapshot.message ??
                "Microphone access is not ready. Check Windows privacy + audio device settings."
            );
            setSnapshot((prev) => ({ ...prev, state: "error" }));
            return;
          }
        }

        await ensureDictationModelReady();
        setError(null);
        setIsBusy(true);
        await startDictation(mode, controlMode);
      } catch (runErr) {
        const message = runErr instanceof Error ? runErr.message : "Unable to start dictation";
        if (message.includes("not installed as a local model artifact")) {
          try {
            await refreshPhase3Data();
          } catch {
            // ignore refresh failures and keep original error
          }
        }
        setError(message);
        setSnapshot((prev) => ({ ...prev, state: "error" }));
      } finally {
        setTimeout(() => setIsBusy(false), 800);
      }
    },
    [ensureDictationModelReady, refreshPhase3Data, runWebFixtureDemo, tauriAvailable]
  );

  const stopDictation = useCallback(async () => {
    if (!tauriAvailable) {
      clearWebTimers();
      setSnapshot((prev) => ({ ...prev, state: "idle", lastPartial: null }));
      setIsBusy(false);
      return;
    }
    try {
      await stopDictationCommand();
      setIsBusy(false);
    } catch (cancelErr) {
      setError(cancelErr instanceof Error ? cancelErr.message : "Unable to stop dictation");
    }
  }, [clearWebTimers, tauriAvailable]);

  const applyHotkeyAction = useCallback(
    async (action: "toggleDictation" | "pushToTalk", phase: "pressed" | "released" | "triggered") => {
      if (tauriAvailable) {
        try {
          await triggerHotkeyAction(action, phase);
        } catch (hotkeyErr) {
          setError(hotkeyErr instanceof Error ? hotkeyErr.message : "Hotkey action failed");
        }
        return;
      }

      if (action === "toggleDictation" && phase === "triggered") {
        if (snapshot.state === "listening" || snapshot.state === "transcribing") {
          await stopDictation();
        } else {
          await runDictation("fixture");
        }
      }

      if (action === "pushToTalk" && phase === "pressed") {
        await runDictation("fixture");
      }
      if (action === "pushToTalk" && phase === "released") {
        await stopDictation();
      }
    },
    [runDictation, snapshot.state, stopDictation, tauriAvailable]
  );

  const setVadThreshold = useCallback(
    async (value: number) => {
      const clampedThreshold = clampVadThreshold(value);
      const nextSettings = { ...settings, vadThreshold: clampedThreshold };
      setSettings(nextSettings);
      if (!tauriAvailable) {
        return true;
      }
      try {
        setSettings(normalizeSettings(await updateSettings(nextSettings)));
      } catch (persistErr) {
        setError(persistErr instanceof Error ? persistErr.message : "Failed to save settings");
      }
    },
    [settings, tauriAvailable]
  );

  const resetVadThreshold = useCallback(async () => {
    await setVadThreshold(RECOMMENDED_VAD_THRESHOLD);
  }, [setVadThreshold]);

  const setMaxUtteranceMs = useCallback(
    async (value: number) => {
      const nextSettings = normalizeSettings({ ...settings, maxUtteranceMs: value });
      setSettings(nextSettings);
      if (!tauriAvailable) {
        return;
      }
      try {
        setSettings(normalizeSettings(await updateSettings(nextSettings)));
      } catch (persistErr) {
        setError(
          persistErr instanceof Error ? persistErr.message : "Failed to save max utterance setting"
        );
      }
    },
    [settings, tauriAvailable]
  );

  const setReleaseTailMs = useCallback(
    async (value: number) => {
      const nextSettings = normalizeSettings({ ...settings, releaseTailMs: value });
      setSettings(nextSettings);
      if (!tauriAvailable) {
        return;
      }
      try {
        setSettings(normalizeSettings(await updateSettings(nextSettings)));
      } catch (persistErr) {
        setError(
          persistErr instanceof Error ? persistErr.message : "Failed to save release tail setting"
        );
      }
    },
    [settings, tauriAvailable]
  );

  const setDecodeMode = useCallback(
    async (mode: DecodeMode) => {
      const nextSettings = normalizeSettings({ ...settings, decodeMode: mode });
      setSettings(nextSettings);
      if (!tauriAvailable) {
        return;
      }
      try {
        setSettings(normalizeSettings(await updateSettings(nextSettings)));
      } catch (persistErr) {
        setError(persistErr instanceof Error ? persistErr.message : "Failed to save decode mode");
      }
    },
    [settings, tauriAvailable]
  );

  const setMicVolumeGuard = useCallback(
    async (mode: MicVolumeGuardMode) => {
      const nextSettings = normalizeSettings({ ...settings, micVolumeGuard: mode });
      setSettings(nextSettings);
      if (!tauriAvailable) {
        return;
      }
      try {
        setSettings(normalizeSettings(await updateSettings(nextSettings)));
      } catch (persistErr) {
        setError(persistErr instanceof Error ? persistErr.message : "Failed to save mic volume guard");
      }
    },
    [settings, tauriAvailable]
  );

  const setTheme = useCallback(
    async (preference: ThemePreference) => {
      const nextSettings = normalizeSettings({ ...settings, theme: preference });
      setSettings(nextSettings);
      if (!tauriAvailable) {
        return;
      }
      try {
        setSettings(normalizeSettings(await updateSettings(nextSettings)));
      } catch (persistErr) {
        setError(persistErr instanceof Error ? persistErr.message : "Failed to save theme");
      }
    },
    [settings, tauriAvailable]
  );

  const setFormatProfile = useCallback(
    async (profile: FormatProfile) => {
      setSettings((prev) => ({ ...prev, formatProfile: profile }));
      if (!tauriAvailable) {
        return;
      }
      try {
        setSettings(normalizeSettings(await setFormatProfileCommand(profile)));
      } catch (persistErr) {
        if (parseProRequiredFeature(persistErr)) {
          setError("PRO_REQUIRED:format_profile");
          return;
        }
        setError(persistErr instanceof Error ? persistErr.message : "Failed to save format profile.");
      }
    },
    [tauriAvailable]
  );

  const setDomainPacks = useCallback(
    async (packs: DomainPackId[]) => {
      setSettings((prev) => ({ ...prev, activeDomainPacks: packs }));
      if (!tauriAvailable) {
        return;
      }
      try {
        setSettings(normalizeSettings(await setActiveDomainPacksCommand(packs)));
      } catch (persistErr) {
        if (parseProRequiredFeature(persistErr)) {
          setError("PRO_REQUIRED:domain_packs");
          return;
        }
        setError(persistErr instanceof Error ? persistErr.message : "Failed to save domain packs.");
      }
    },
    [tauriAvailable]
  );

  const setAppProfiles = useCallback(
    async (overrides: AppProfileOverrides) => {
      setSettings((prev) => ({ ...prev, appProfileOverrides: overrides }));
      if (!tauriAvailable) {
        return;
      }
      try {
        setSettings(normalizeSettings(await setAppProfileOverridesCommand(overrides)));
      } catch (persistErr) {
        if (parseProRequiredFeature(persistErr)) {
          setError("PRO_REQUIRED:app_profiles");
          return;
        }
        setError(persistErr instanceof Error ? persistErr.message : "Failed to save app profiles.");
      }
    },
    [tauriAvailable]
  );

  const setCodeModeSettings = useCallback(
    async (codeMode: CodeModeSettings) => {
      setSettings((prev) => ({ ...prev, codeMode }));
      if (!tauriAvailable) {
        return;
      }
      try {
        setSettings(normalizeSettings(await setCodeModeSettingsCommand(codeMode)));
      } catch (persistErr) {
        if (parseProRequiredFeature(persistErr)) {
          setError("PRO_REQUIRED:code_mode");
          return;
        }
        setError(persistErr instanceof Error ? persistErr.message : "Failed to save code mode.");
      }
    },
    [tauriAvailable]
  );

  /** Selects a polish profile with ONE atomic backend write (plan 010).
   * Errors are rethrown instead of swallowed so the caller can detect an
   * older backend (unknown command) and run the deprecated multi-write
   * fallback. Selecting a profile always clears the customized flag. */
  const setDictationProfile = useCallback(
    async (profile: PolishProfile) => {
      setSettings((prev) => ({ ...prev, polishProfile: profile, polishProfileCustomized: false }));
      if (!tauriAvailable) {
        return;
      }
      setSettings(normalizeSettings(await setDictationProfileCommand(profile)));
    },
    [tauriAvailable]
  );

  const setProPostProcessingEnabled = useCallback(
    async (enabled: boolean) => {
      setSettings((prev) => ({ ...prev, proPostProcessingEnabled: enabled }));
      if (!tauriAvailable) {
        return;
      }
      try {
        setSettings(normalizeSettings(await setProPostProcessingEnabledCommand(enabled)));
      } catch (persistErr) {
        if (parseProRequiredFeature(persistErr)) {
          setError("PRO_REQUIRED:post_processing");
          return;
        }
        setError(
          persistErr instanceof Error ? persistErr.message : "Failed to save post-processing setting."
        );
      }
    },
    [tauriAvailable]
  );

  const setInputDevice = useCallback(
    async (deviceName: string | null) => {
      const nextSettings = { ...settings, inputDevice: deviceName };
      setSettings(nextSettings);
      if (!tauriAvailable) {
        return;
      }
      try {
        setSettings(normalizeSettings(await updateSettings(nextSettings)));
        if (snapshot.state === "listening") {
          await stopMicLevelMonitor();
          await startMicLevelMonitor();
        }
        await refreshInputDevices();
      } catch (persistErr) {
        setError(persistErr instanceof Error ? persistErr.message : "Failed to update input device");
      }
    },
    [refreshInputDevices, settings, snapshot.state, tauriAvailable]
  );

  const switchToRecommendedInput = useCallback(async () => {
    const candidate = findRecommendedInputDevice(inputDevices, settings.inputDevice);
    if (!candidate) {
      setError("No higher-quality microphone candidate was detected. Try a wired or built-in mic.");
      return;
    }
    await setInputDevice(candidate);
  }, [inputDevices, setInputDevice, settings.inputDevice]);

  const setPreferClipboardFallback = useCallback(
    async (enabled: boolean) => {
      const nextSettings = { ...settings, preferClipboardFallback: enabled };
      setSettings(nextSettings);
      if (!tauriAvailable) {
        return;
      }
      try {
        setSettings(normalizeSettings(await updateSettings(nextSettings)));
      } catch (persistErr) {
        setError(persistErr instanceof Error ? persistErr.message : "Failed to save insertion preference");
      }
    },
    [settings, tauriAvailable]
  );

  const completeOnboarding = useCallback(async () => {
    const nextSettings = { ...settings, onboardingCompleted: true };
    setSettings(nextSettings);
    if (!tauriAvailable) {
      return;
    }
    try {
      setSettings(normalizeSettings(await updateSettings(nextSettings)));
    } catch {
      // Non-fatal: the flow simply shows again next launch.
    }
  }, [settings, tauriAvailable]);

  // Help → "Run setup again": clears the flag so the flow re-mounts fresh.
  const restartOnboarding = useCallback(async () => {
    const nextSettings = { ...settings, onboardingCompleted: false };
    setSettings(nextSettings);
    if (!tauriAvailable) {
      return;
    }
    try {
      setSettings(normalizeSettings(await updateSettings(nextSettings)));
    } catch {
      // Non-fatal: the local state already reopened the flow.
    }
  }, [settings, tauriAvailable]);

  const setSpokenEditCommands = useCallback(
    async (enabled: boolean) => {
      const nextSettings = { ...settings, spokenEditCommands: enabled };
      setSettings(nextSettings);
      if (!tauriAvailable) {
        return;
      }
      try {
        setSettings(normalizeSettings(await updateSettings(nextSettings)));
      } catch (persistErr) {
        setError(persistErr instanceof Error ? persistErr.message : "Failed to save spoken commands setting");
      }
    },
    [settings, tauriAvailable]
  );

  const [polishModelProgress, setPolishModelProgress] = useState<PolishModelProgress | null>(null);

  const setLlmPolishEnabled = useCallback(
    async (enabled: boolean) => {
      const nextSettings = { ...settings, llmPolishEnabled: enabled };
      setSettings(nextSettings);
      if (!tauriAvailable) {
        return;
      }
      try {
        setSettings(normalizeSettings(await updateSettings(nextSettings)));
      } catch (persistErr) {
        setError(persistErr instanceof Error ? persistErr.message : "Failed to save AI polish preference");
        return false;
      }
      // Enabling AI polish needs the ~1 GB model. Kick off the (idempotent,
      // single-flighted) download; progress arrives via the listener below and
      // renders in the settings panel. A fast no-op when it is already present.
      if (enabled) {
        try {
          const status = await getPolishModelStatus();
          if (!status.present && !status.downloading) {
            setPolishModelProgress({ downloaded: 0, total: 0, done: false, error: null });
            void downloadPolishModel().catch(() => undefined);
          }
        } catch {
          // Status check is best-effort; the worker also degrades gracefully.
        }
      }
      return true;
    },
    [settings, tauriAvailable]
  );

  useEffect(() => {
    if (!tauriAvailable) {
      return;
    }
    let active = true;
    let unlisten: (() => void) | undefined;
    void listenVoicewavePolishModelProgress((payload) => {
      setPolishModelProgress(payload);
    }).then((fn) => {
      if (active) {
        unlisten = fn;
      } else {
        fn();
      }
    });
    return () => {
      active = false;
      unlisten?.();
    };
  }, [tauriAvailable]);

  const setDiagnosticsOptIn = useCallback(
    async (enabled: boolean) => {
      setSettings((prev) => ({ ...prev, diagnosticsOptIn: enabled }));
      if (!tauriAvailable) {
        setDiagnosticsStatus((prev) => ({ ...prev, optIn: enabled }));
        return;
      }
      try {
        const status = await setDiagnosticsOptInCommand(enabled);
        setDiagnosticsStatus(status);
        setSettings((prev) => ({ ...prev, diagnosticsOptIn: status.optIn }));
      } catch (persistErr) {
        setError(
          persistErr instanceof Error ? persistErr.message : "Failed to update diagnostics opt-in"
        );
      }
    },
    [tauriAvailable]
  );

  const setAnonymousUsageOptIn = useCallback(
    async (enabled: boolean) => {
      const nextSettings = { ...settings, anonymousUsageOptIn: enabled };
      setSettings(nextSettings);
      if (!tauriAvailable) {
        return;
      }
      try {
        setSettings(normalizeSettings(await updateSettings(nextSettings)));
      } catch (persistErr) {
        setError(
          persistErr instanceof Error
            ? persistErr.message
            : "Failed to update anonymous usage sharing"
        );
      }
    },
    [settings, tauriAvailable]
  );

  const exportDiagnosticsBundle = useCallback(async () => {
    if (!tauriAvailable) {
      setError("Diagnostics export requires desktop runtime (tauri).");
      return;
    }
    try {
      setError(null);
      const result = await exportDiagnosticsBundleCommand();
      setLastDiagnosticsExport(result);
      setDiagnosticsStatus(await getDiagnosticsStatus());
    } catch (diagnosticErr) {
      setError(
        diagnosticErr instanceof Error ? diagnosticErr.message : "Diagnostics export failed"
      );
    }
  }, [tauriAvailable]);

  const updateHotkeys = useCallback(
    async (_config: HotkeyConfig) => {
      const lockedConfig = {
        toggle: LOCKED_TOGGLE_HOTKEY,
        pushToTalk: LOCKED_PUSH_TO_TALK_HOTKEY
      };
      setHotkeys((prev) => ({ ...prev, config: lockedConfig }));
      setSettings((prev) => ({
        ...prev,
        toggleHotkey: LOCKED_TOGGLE_HOTKEY,
        pushToTalkHotkey: LOCKED_PUSH_TO_TALK_HOTKEY
      }));
      if (!tauriAvailable) {
        return;
      }
      try {
        setHotkeys(await updateHotkeyConfig(lockedConfig));
      } catch (hotkeyErr) {
        setError(hotkeyErr instanceof Error ? hotkeyErr.message : "Failed to update hotkeys");
      }
    },
    [tauriAvailable]
  );

  const requestMicAccess = useCallback(async () => {
    if (!tauriAvailable) {
      setPermissions((prev) => ({ ...prev, microphone: "granted", message: "Web fallback has no OS bridge." }));
      return;
    }
    try {
      setPermissions(await requestMicrophoneAccess());
    } catch (permissionErr) {
      setError(permissionErr instanceof Error ? permissionErr.message : "Failed to request microphone access");
    }
  }, [tauriAvailable]);

  const runAudioQualityDiagnostic = useCallback(async (durationMs = 10_000) => {
    if (!tauriAvailable) {
      setError("Audio quality diagnostics require desktop runtime (tauri).");
      return;
    }
    try {
      setError(null);
      const report = await runAudioQualityDiagnosticCommand(durationMs);
      setAudioQualityReport(report);
    } catch (diagnosticErr) {
      setError(diagnosticErr instanceof Error ? diagnosticErr.message : "Audio quality check failed");
    }
  }, [tauriAvailable]);

  const undoInsertion = useCallback(async () => {
    if (!tauriAvailable) {
      setError("Undo is available in desktop runtime.");
      return;
    }
    try {
      const result = await undoLastInsertion();
      setError(!result.success && result.message ? result.message : null);
      await refreshRecentInsertions();
    } catch (undoErr) {
      setError(undoErr instanceof Error ? undoErr.message : "Undo failed");
    }
  }, [refreshRecentInsertions, tauriAvailable]);

  const insertFinalTranscript = useCallback(async () => {
    if (!snapshot.lastFinal) {
      setError("No final transcript available to insert.");
      return;
    }
    if (!tauriAvailable) {
      setLastInsertion({
        success: true,
        method: "clipboardOnly",
        message: "Web mode insertion is simulated.",
        targetApp: null,
        transactionId: `web-${Date.now()}`,
        undoAvailable: false
      });
      return;
    }
    try {
      const result = await insertText({
        text: snapshot.lastFinal,
        targetApp: null,
        preferClipboard: settings.preferClipboardFallback
      });
      setLastInsertion(result);
      await refreshRecentInsertions();
      await refreshPhase3Data();
    } catch (insertErr) {
      setError(insertErr instanceof Error ? insertErr.message : "Manual insertion failed");
    }
  }, [refreshPhase3Data, refreshRecentInsertions, settings.preferClipboardFallback, snapshot.lastFinal, tauriAvailable]);

  const installModel = useCallback(
    async (modelId: string) => {
      if (!tauriAvailable) {
        setError("Desktop runtime is required to download models. Run npm run tauri:dev.");
        return;
      }
      const catalogRow = modelCatalog.find((item) => item.modelId === modelId);
      const resumable =
        catalogRow?.format !== "faster-whisper" && !isFasterWhisperModelId(modelId);
      try {
        setModelStatuses((prev) => ({
          ...prev,
          [modelId]: {
            modelId,
            state: "downloading",
            progress: 5,
            active: false,
            installed: false,
            message: "Preparing model download…",
            installedModel: null,
            downloadedBytes: 0,
            totalBytes: catalogRow?.sizeBytes ?? null,
            resumable
          }
        }));
        const status = await downloadModel({ modelId });
        setModelStatuses((prev) => ({ ...prev, [modelId]: status }));
        await refreshPhase3Data();
      } catch (modelErr) {
        const message = modelErr instanceof Error ? modelErr.message : "Model install failed";
        setError(message);
        // Belt and braces: even if the backend never emitted a terminal status
        // (or the IPC call itself rejected), the row must leave "downloading"
        // so Retry/Cancel appear instead of a frozen progress bar. A terminal
        // status the backend already emitted is kept — its message is the
        // friendly one.
        setModelStatuses((prev) => {
          const current = prev[modelId];
          if (current?.state === "failed" || current?.state === "cancelled") {
            return prev;
          }
          return {
            ...prev,
            [modelId]: {
              ...(current ?? {
                modelId,
                active: false,
                installed: false,
                installedModel: null,
                totalBytes: catalogRow?.sizeBytes ?? null,
                downloadedBytes: 0
              }),
              state: "failed",
              progress: 0,
              message,
              resumable: false
            }
          };
        });
      }
    },
    [modelCatalog, refreshPhase3Data, settings.activeModel, tauriAvailable]
  );

  const cancelModelInstall = useCallback(async (modelId: string) => {
    if (!tauriAvailable) {
      setModelStatuses((prev) => ({
        ...prev,
        [modelId]: {
          ...(prev[modelId] ?? {
            modelId,
            active: false,
            installed: false,
            installedModel: null,
            progress: 0,
            resumable: true
          }),
          state: "cancelled",
          message: "Download cancelled.",
          resumable: true
        }
      }));
      return;
    }
    try {
      const status = await cancelModelDownload(modelId);
      setModelStatuses((prev) => ({ ...prev, [modelId]: status }));
    } catch (modelErr) {
      setError(modelErr instanceof Error ? modelErr.message : "Cancel failed");
    }
  }, [tauriAvailable]);

  const pauseModelInstall = useCallback(async (modelId: string) => {
    if (!tauriAvailable) {
      setModelStatuses((prev) => ({
        ...prev,
        [modelId]: {
          ...(prev[modelId] ?? {
            modelId,
            active: false,
            installed: false,
            installedModel: null,
            progress: 0,
            resumable: true
          }),
          state: "paused",
          message: "Paused in web simulation.",
          resumable: true
        }
      }));
      return;
    }
    try {
      const status = await pauseModelDownload(modelId);
      setModelStatuses((prev) => ({ ...prev, [modelId]: status }));
    } catch (modelErr) {
      setError(modelErr instanceof Error ? modelErr.message : "Pause failed");
    }
  }, [tauriAvailable]);

  const resumeModelInstall = useCallback(async (modelId: string) => {
    if (!tauriAvailable) {
      setModelStatuses((prev) => ({
        ...prev,
        [modelId]: {
          ...(prev[modelId] ?? {
            modelId,
            active: false,
            installed: false,
            installedModel: null,
            progress: 0,
            resumable: true
          }),
          state: "downloading",
          message: "Resumed in web simulation.",
          resumable: true
        }
      }));
      return;
    }
    try {
      const status = await resumeModelDownload(modelId);
      setModelStatuses((prev) => ({ ...prev, [modelId]: status }));
      await refreshPhase3Data();
    } catch (modelErr) {
      setError(modelErr instanceof Error ? modelErr.message : "Resume failed");
    }
  }, [refreshPhase3Data, tauriAvailable]);

  const makeModelActive = useCallback(async (modelId: string) => {
    if (!tauriAvailable) {
      setSettings((prev) => ({ ...prev, activeModel: modelId }));
      setSnapshot((prev) => ({ ...prev, activeModel: modelId }));
      setModelStatuses((prev) => {
        const next = { ...prev };
        for (const key of Object.keys(next)) {
          next[key] = { ...next[key], active: key === modelId };
        }
        return next;
      });
      return;
    }
    try {
      const nextSettings = normalizeSettings(await setActiveModel(modelId));
      setSettings(nextSettings);
      setSnapshot((prev) => ({ ...prev, activeModel: nextSettings.activeModel }));
      await refreshPhase3Data();
    } catch (modelErr) {
      setError(modelErr instanceof Error ? modelErr.message : "Failed to switch active model");
    }
  }, [refreshPhase3Data, tauriAvailable]);

  const runBenchmarkAndRecommend = useCallback(async () => {
    if (!tauriAvailable) {
      const run: BenchmarkRun = {
        startedAtUtcMs: Date.now() - 1200,
        completedAtUtcMs: Date.now(),
        rows: [
          { modelId: "fw-small.en", runs: 3, p50LatencyMs: 260, p95LatencyMs: 420, averageRtf: 0.41 },
          { modelId: "fw-large-v3-turbo", runs: 3, p50LatencyMs: 510, p95LatencyMs: 830, averageRtf: 0.79 }
        ]
      };
      setBenchmarkResults(run);
      setModelRecommendation({
        modelId: "fw-small.en",
        reason: "Best model under configured latency and RTF gates.",
        p95LatencyMs: 420,
        averageRtf: 0.41,
        meetsLatencyGate: true,
        meetsRtfGate: true
      });
      return;
    }
    try {
      if (installedModels.length === 0) {
        setError("Install at least one model before running benchmark.");
        return;
      }

      const benchmarkRequest = {
        modelIds: installedModels.map((model) => model.modelId)
      };
      const run = await runModelBenchmark(benchmarkRequest);
      setBenchmarkResults(run);

      const recommendation = await recommendModel();
      setModelRecommendation(recommendation);

      const recommendedInstalled = installedModels.some(
        (model) => model.modelId === recommendation.modelId
      );
      if (recommendedInstalled && recommendation.modelId !== settings.activeModel) {
        const nextSettings = normalizeSettings(await setActiveModel(recommendation.modelId));
        setSettings(nextSettings);
        setSnapshot((prev) => ({ ...prev, activeModel: nextSettings.activeModel }));
        await refreshPhase3Data(nextSettings.activeModel);
      }
    } catch (benchmarkErr) {
      setError(benchmarkErr instanceof Error ? benchmarkErr.message : "Benchmark flow failed");
    }
  }, [installedModels, refreshPhase3Data, settings.activeModel, tauriAvailable]);

  const updateRetentionPolicy = useCallback(async (policy: RetentionPolicy) => {
    if (!tauriAvailable) {
      setHistoryPolicy(policy);
      if (policy === "off") {
        setSessionHistory([]);
      }
      return;
    }
    try {
      setHistoryPolicy(await setHistoryRetention(policy));
      await refreshPhase3Data();
    } catch (historyErr) {
      setError(historyErr instanceof Error ? historyErr.message : "Failed to update retention");
    }
  }, [refreshPhase3Data, tauriAvailable]);

  const pruneHistory = useCallback(async () => {
    if (!tauriAvailable) {
      setSessionHistory((prev) => prev.slice(0, 20));
      return;
    }
    try {
      await pruneHistoryNow();
      await refreshPhase3Data();
    } catch (historyErr) {
      setError(historyErr instanceof Error ? historyErr.message : "Failed to prune history");
    }
  }, [refreshPhase3Data, tauriAvailable]);

  const clearSessionHistory = useCallback(async () => {
    if (!tauriAvailable) {
      setSessionHistory([]);
      return;
    }
    try {
      await clearHistory();
      await refreshPhase3Data();
    } catch (historyErr) {
      setError(historyErr instanceof Error ? historyErr.message : "Failed to clear history");
    }
  }, [refreshPhase3Data, tauriAvailable]);

  const searchHistory = useCallback(
    async (query: string, tags?: string[] | null, starred?: boolean | null) => {
      if (!tauriAvailable) {
        const localQuery = query.trim().toLowerCase();
        const filtered = sessionHistory.filter((row) => {
          const textMatch =
            localQuery.length === 0 ||
            row.preview.toLowerCase().includes(localQuery) ||
            row.source.toLowerCase().includes(localQuery);
          const tagMatch =
            !tags || tags.length === 0
              ? true
              : tags.every((tag) => row.tags.some((existing) => existing.toLowerCase() === tag.toLowerCase()));
          const starMatch = starred == null ? true : row.starred === starred;
          return textMatch && tagMatch && starMatch;
        });
        setSessionHistory(filtered);
        return filtered;
      }
      try {
        const rows = await searchSessionHistoryCommand(query, tags, starred);
        setSessionHistory(rows);
        return rows;
      } catch (historyErr) {
        if (parseProRequiredFeature(historyErr)) {
          setError("PRO_REQUIRED:advanced_history");
          return [];
        }
        setError(historyErr instanceof Error ? historyErr.message : "Failed to search history.");
        return [];
      }
    },
    [sessionHistory, tauriAvailable]
  );

  const addSessionTag = useCallback(
    async (recordId: string, tag: string) => {
      if (!tauriAvailable) {
        setSessionHistory((prev) =>
          prev.map((row) =>
            row.recordId === recordId && !row.tags.includes(tag)
              ? { ...row, tags: [...row.tags, tag] }
              : row
          )
        );
        return;
      }
      try {
        const updated = await tagSessionCommand(recordId, tag);
        setSessionHistory((prev) => prev.map((row) => (row.recordId === recordId ? updated : row)));
      } catch (historyErr) {
        if (parseProRequiredFeature(historyErr)) {
          setError("PRO_REQUIRED:advanced_history");
          return;
        }
        setError(historyErr instanceof Error ? historyErr.message : "Failed to tag session.");
      }
    },
    [tauriAvailable]
  );

  const setSessionStarred = useCallback(
    async (recordId: string, starred: boolean) => {
      if (!tauriAvailable) {
        setSessionHistory((prev) =>
          prev.map((row) => (row.recordId === recordId ? { ...row, starred } : row))
        );
        return;
      }
      try {
        const updated = await toggleStarSessionCommand(recordId, starred);
        setSessionHistory((prev) => prev.map((row) => (row.recordId === recordId ? updated : row)));
      } catch (historyErr) {
        if (parseProRequiredFeature(historyErr)) {
          setError("PRO_REQUIRED:advanced_history");
          return;
        }
        setError(historyErr instanceof Error ? historyErr.message : "Failed to star session.");
      }
    },
    [tauriAvailable]
  );

  const exportHistoryPreset = useCallback(
    async (preset: HistoryExportPreset) => {
      if (!tauriAvailable) {
        const fallback: HistoryExportResult = {
          preset,
          recordCount: sessionHistory.length,
          content: sessionHistory.map((row) => row.preview).join("\n")
        };
        setLastHistoryExport(fallback);
        return fallback;
      }
      try {
        const result = await exportSessionHistoryPreset(preset);
        setLastHistoryExport(result);
        return result;
      } catch (historyErr) {
        if (parseProRequiredFeature(historyErr)) {
          setError("PRO_REQUIRED:advanced_history");
          return null;
        }
        setError(historyErr instanceof Error ? historyErr.message : "Failed to export history.");
        return null;
      }
    },
    [sessionHistory, tauriAvailable]
  );

  const approveDictionaryQueueEntry = useCallback(async (entryId: string, normalizedText?: string) => {
    if (!tauriAvailable) {
      const item = dictionaryQueue.find((entry) => entry.entryId === entryId);
      if (!item) {
        return;
      }
      setDictionaryQueue((prev) => prev.filter((entry) => entry.entryId !== entryId));
      setDictionaryTerms((prev) => [...prev, { termId: `dt-${Date.now()}`, term: (normalizedText ?? item.term).trim(), source: "queue-approval", createdAtUtcMs: Date.now() }]);
      return;
    }
    try {
      await approveDictionaryEntry(entryId, normalizedText);
      await refreshDictionary();
    } catch (dictionaryErr) {
      setError(dictionaryErr instanceof Error ? dictionaryErr.message : "Failed to approve term");
      throw dictionaryErr;
    }
  }, [dictionaryQueue, refreshDictionary, tauriAvailable]);

  const rejectDictionaryQueueEntry = useCallback(async (entryId: string) => {
    if (!tauriAvailable) {
      setDictionaryQueue((prev) => prev.filter((entry) => entry.entryId !== entryId));
      return;
    }
    try {
      await rejectDictionaryEntry(entryId);
      await refreshDictionary();
    } catch (dictionaryErr) {
      setError(dictionaryErr instanceof Error ? dictionaryErr.message : "Failed to reject term");
      throw dictionaryErr;
    }
  }, [refreshDictionary, tauriAvailable]);

  const deleteDictionaryTerm = useCallback(async (termId: string) => {
    if (!tauriAvailable) {
      setDictionaryTerms((prev) => prev.filter((term) => term.termId !== termId));
      return;
    }
    try {
      await removeDictionaryTerm(termId);
      await refreshDictionary();
    } catch (dictionaryErr) {
      setError(dictionaryErr instanceof Error ? dictionaryErr.message : "Failed to remove term");
      throw dictionaryErr;
    }
  }, [refreshDictionary, tauriAvailable]);

  const addDictionaryTerm = useCallback(
    async (term: string) => {
      const normalized = term.trim();
      if (!normalized) {
        return;
      }
      if (!tauriAvailable) {
        setDictionaryTerms((prev) => [
          ...prev,
          {
            termId: `dt-${Date.now()}`,
            term: normalized,
            source: "manual-add",
            createdAtUtcMs: Date.now()
          }
        ]);
        setDictionaryQueue((prev) =>
          prev.filter((entry) => entry.term.toLowerCase() !== normalized.toLowerCase())
        );
        return;
      }

      try {
        await addDictionaryTermCommand(normalized);
        await refreshDictionary();
      } catch (dictionaryErr) {
        setError(dictionaryErr instanceof Error ? dictionaryErr.message : "Failed to add term");
        throw dictionaryErr;
      }
    },
    [refreshDictionary, tauriAvailable]
  );

  const exportDictionary = useCallback(async (): Promise<DictionaryExport> => {
    return exportDictionaryCommand();
  }, []);

  const importDictionary = useCallback(
    async (payload: string): Promise<DictionaryImportSummary> => {
      const summary = await importDictionaryCommand(payload);
      await refreshDictionary();
      return summary;
    },
    [refreshDictionary]
  );

  const addVoiceSnippet = useCallback(async (trigger: string, expansion: string) => {
    if (!tauriAvailable) {
      const now = Date.now();
      const normalizedTrigger = trigger.trim().replace(/\s+/gu, " ").normalize("NFC").toLowerCase();
      setVoiceSnippets((current) => [
        ...current,
        {
          snippetId: `vs-web-${now}`,
          trigger: trigger.trim().replace(/\s+/gu, " ").normalize("NFC"),
          normalizedTrigger,
          expansion: expansion.replace(/\r\n/g, "\n"),
          createdAtUtcMs: now,
          updatedAtUtcMs: now
        }
      ]);
      return;
    }
    await addVoiceSnippetCommand(trigger, expansion);
    await refreshVoiceSnippets();
  }, [refreshVoiceSnippets, tauriAvailable]);

  const updateVoiceSnippet = useCallback(async (
    snippetId: string,
    trigger: string,
    expansion: string
  ) => {
    if (!tauriAvailable) {
      const now = Date.now();
      setVoiceSnippets((current) => current.map((snippet) =>
        snippet.snippetId === snippetId
          ? {
              ...snippet,
              trigger: trigger.trim().replace(/\s+/gu, " ").normalize("NFC"),
              normalizedTrigger: trigger.trim().replace(/\s+/gu, " ").normalize("NFC").toLowerCase(),
              expansion: expansion.replace(/\r\n/g, "\n"),
              updatedAtUtcMs: now
            }
          : snippet
      ));
      return;
    }
    await updateVoiceSnippetCommand(snippetId, trigger, expansion);
    await refreshVoiceSnippets();
  }, [refreshVoiceSnippets, tauriAvailable]);

  const deleteVoiceSnippet = useCallback(async (snippetId: string) => {
    if (!tauriAvailable) {
      setVoiceSnippets((current) => current.filter((snippet) => snippet.snippetId !== snippetId));
      return;
    }
    await removeVoiceSnippetCommand(snippetId);
    await refreshVoiceSnippets();
  }, [refreshVoiceSnippets, tauriAvailable]);

  useEffect(() => {
    if (!tauriAvailable) {
      setSessionHistory([
        {
          recordId: "hist-web-1",
          timestampUtcMs: Date.now() - 10000,
          preview: "Phase three panel wiring is ready for desktop integration.",
          method: "clipboardOnly",
          success: true,
          source: "insertion",
          message: "Web-mode simulation",
          tags: [],
          starred: false
        }
      ]);
      setEntitlement(fallbackEntitlement);
      setDictionaryQueue([{ entryId: "dq-web-1", term: "VoiceWave", sourcePreview: "Prototype note", createdAtUtcMs: Date.now() - 5000 }]);
      setDictionaryTerms([{ termId: "dt-web-1", term: "whisper.cpp", source: "seed", createdAtUtcMs: Date.now() - 20000 }]);
      setVoiceSnippets([]);
      return;
    }

    let stateUnlisten: (() => void) | null = null;
    let transcriptUnlisten: (() => void) | null = null;
    let insertionUnlisten: (() => void) | null = null;
    let permissionUnlisten: (() => void) | null = null;
    let hotkeyUnlisten: (() => void) | null = null;
    let modelUnlisten: (() => void) | null = null;
    let audioQualityUnlisten: (() => void) | null = null;
    let latencyUnlisten: (() => void) | null = null;
    let historyUnlisten: (() => void) | null = null;

    void (async () => {
      try {
        const [
          loadedSnapshot,
          loadedSettings,
          loadedHotkeys,
          permissionSnapshot,
          insertionRows,
          devices,
          diagnostics,
          entitlementSnapshot
        ] = await Promise.all([
          loadSnapshot(),
          loadSettings(),
          loadHotkeyConfig(),
          getPermissionSnapshot(),
          getRecentInsertions(8),
          listInputDevices(),
          getDiagnosticsStatus(),
          getEntitlementSnapshot()
        ]);
        const safeLoadedSettings = normalizeSettings({
          ...loadedSettings,
          diagnosticsOptIn: diagnostics.optIn
        });
        setSnapshot(loadedSnapshot);
        setSettings(safeLoadedSettings);
        setDiagnosticsStatus(diagnostics);
        setHotkeys(loadedHotkeys);
        setPermissions(permissionSnapshot);
        setRecentInsertions(insertionRows);
        setInputDevices(devices);
        setEntitlement(entitlementSnapshot);
        await refreshPhase3Data(safeLoadedSettings.activeModel);
        try {
          setHistoryPolicy(await getHistoryRetention());
        } catch {
          // Older backends without the getter: keep the optimistic default.
        }
      } catch (loadErr) {
        setError(loadErr instanceof Error ? loadErr.message : "Failed to initialize VoiceWave runtime.");
      }

      stateUnlisten = await listenVoicewaveState(({ controlMode, message, state }) => {
        setSnapshot((prev) => ({
          ...prev,
          state,
          controlMode: controlMode ?? (state === "idle" ? null : prev.controlMode)
        }));
        if (state === "error" && message) {
          setError(message);
        } else if (state !== "error") {
          setError(null);
        }
      });

      transcriptUnlisten = await listenVoicewaveTranscript((event) => {
        setSnapshot((prev) => ({
          ...prev,
          lastPartial: event.isFinal ? prev.lastPartial : event.text,
          lastFinal: event.isFinal ? event.text : prev.lastFinal
        }));
        if (event.isFinal && event.text.trim()) {
          void trackAnonymousUsage(settingsRef.current.anonymousUsageOptIn).catch(() => undefined);
        }
      });

      insertionUnlisten = await listenVoicewaveInsertion((result) => {
        setLastInsertion(result);
        void refreshRecentInsertions();
        void refreshPhase3Data();
      });

      permissionUnlisten = await listenVoicewavePermission((payload) => {
        setPermissions(payload);
      });

      hotkeyUnlisten = await listenVoicewaveHotkey((payload) => {
        setLastHotkeyEvent(payload);
      });

      modelUnlisten = await listenVoicewaveModel((payload: ModelEvent) => {
        if (typeof payload.downloadedBytes === "number") {
          const now = Date.now();
          const last = speedSamples.current[payload.modelId];
          if (last && now > last.time && payload.downloadedBytes >= last.bytes) {
            const deltaBytes = payload.downloadedBytes - last.bytes;
            const deltaSeconds = (now - last.time) / 1000;
            const speed = deltaSeconds > 0 ? deltaBytes / deltaSeconds : 0;
            setModelSpeeds((prev) => ({ ...prev, [payload.modelId]: speed }));
          }
          speedSamples.current[payload.modelId] = {
            bytes: payload.downloadedBytes,
            time: now
          };
        }
        setModelStatuses((prev) => ({
          ...prev,
          [payload.modelId]: {
            ...(prev[payload.modelId] ?? {
              modelId: payload.modelId,
              active: settings.activeModel === payload.modelId,
              installed: false,
              installedModel: null,
              progress: 0,
              resumable: true
            }),
            state: payload.state,
            progress: payload.progress,
            message: payload.message ?? null,
            downloadedBytes: payload.downloadedBytes ?? null,
            totalBytes: payload.totalBytes ?? null,
            // ModelEvent carries no `resumable`; faster-whisper downloads never
            // are (no pause/resume in the python runtime).
            resumable: payload.state !== "installed" && !isFasterWhisperModelId(payload.modelId)
          }
        }));
      });

      audioQualityUnlisten = await listenVoicewaveAudioQuality((payload: AudioQualityReport) => {
        setAudioQualityReport(payload);
      });

      latencyUnlisten = await listenVoicewaveLatency((payload: LatencyBreakdownEvent) => {
        setLastLatency(payload);
      });

      historyUnlisten = await listenVoicewaveHistoryUpdated(() => {
        void refreshSessionHistory();
      });
    })();

    return () => {
      if (stateUnlisten) {
        stateUnlisten();
      }
      if (transcriptUnlisten) {
        transcriptUnlisten();
      }
      if (insertionUnlisten) {
        insertionUnlisten();
      }
      if (permissionUnlisten) {
        permissionUnlisten();
      }
      if (hotkeyUnlisten) {
        hotkeyUnlisten();
      }
      if (modelUnlisten) {
        modelUnlisten();
      }
      if (audioQualityUnlisten) {
        audioQualityUnlisten();
      }
      if (latencyUnlisten) {
        latencyUnlisten();
      }
      if (historyUnlisten) {
        historyUnlisten();
      }
      if (tauriAvailable) {
        void stopMicLevelMonitor();
      }
    };
  }, [refreshPhase3Data, refreshRecentInsertions, refreshSessionHistory, settings.activeModel, tauriAvailable]);

  useEffect(() => {
    return () => {
      clearWebTimers();
    };
  }, [clearWebTimers]);

  useEffect(() => {
    const toggleMainKey = getComboMainKey(hotkeys.config.toggle);
    const pushMainKey = getComboMainKey(hotkeys.config.pushToTalk);

    const shouldSuppress = (event: KeyboardEvent): boolean => {
      if (isEditableTarget(event.target)) {
        return false;
      }
      if (
        comboMatchesKeyboardEvent(event, hotkeys.config.toggle) ||
        comboMatchesKeyboardEvent(event, hotkeys.config.pushToTalk)
      ) {
        return true;
      }
      if (
        event.key === " " &&
        (event.ctrlKey || event.altKey || event.metaKey || event.shiftKey)
      ) {
        return true;
      }
      if (toggleMainKey && eventMatchesMainKey(event, toggleMainKey) && (event.ctrlKey || event.altKey || event.metaKey)) {
        return true;
      }
      if (pushMainKey && eventMatchesMainKey(event, pushMainKey) && (event.ctrlKey || event.altKey || event.metaKey)) {
        return true;
      }
      return false;
    };

    const onKeyDown = (event: KeyboardEvent) => {
      if (!shouldSuppress(event)) {
        return;
      }
      event.preventDefault();
      event.stopPropagation();
    };

    const onKeyUp = (event: KeyboardEvent) => {
      if (!shouldSuppress(event)) {
        return;
      }
      event.preventDefault();
      event.stopPropagation();
    };

    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("keyup", onKeyUp);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("keyup", onKeyUp);
    };
  }, [hotkeys.config.pushToTalk, hotkeys.config.toggle]);

  useEffect(() => {
    if (tauriAvailable) {
      return () => {
        pushToTalkLatchedRef.current = false;
      };
    }

    const pushMainKey = getComboMainKey(hotkeys.config.pushToTalk);
    const pushModifierOnly = comboIsModifierOnly(hotkeys.config.pushToTalk);
    const releasePushToTalk = () => {
      if (!pushToTalkLatchedRef.current) {
        return;
      }
      pushToTalkLatchedRef.current = false;
      void applyHotkeyAction("pushToTalk", "released");
    };

    const onKeyDown = (event: KeyboardEvent) => {
      if (isEditableTarget(event.target)) {
        return;
      }
      if (!event.repeat && comboMatchesKeyboardEvent(event, hotkeys.config.toggle)) {
        event.preventDefault();
        void applyHotkeyAction("toggleDictation", "triggered");
        return;
      }
      if (!event.repeat && comboMatchesKeyboardEvent(event, hotkeys.config.pushToTalk)) {
        event.preventDefault();
        if (!pushToTalkLatchedRef.current) {
          pushToTalkLatchedRef.current = true;
          void applyHotkeyAction("pushToTalk", "pressed");
        }
      }
    };

    const onKeyUp = (event: KeyboardEvent) => {
      if (!pushToTalkLatchedRef.current) {
        return;
      }
      if (pushModifierOnly) {
        const key = event.key.toUpperCase();
        const isModifierRelease =
          key === "CONTROL" ||
          key === "CTRL" ||
          key === "META" ||
          key === "OS" ||
          key === "WIN" ||
          key === "WINDOWS";
        if (!isModifierRelease) {
          return;
        }
        if (!comboMatchesKeyboardEvent(event, hotkeys.config.pushToTalk)) {
          event.preventDefault();
          releasePushToTalk();
        }
        return;
      }
      if (pushMainKey && eventMatchesMainKey(event, pushMainKey)) {
        event.preventDefault();
        releasePushToTalk();
      }
    };

    const onWindowBlur = () => {
      releasePushToTalk();
    };

    const onVisibilityChange = () => {
      if (document.hidden) {
        releasePushToTalk();
      }
    };

    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("keyup", onKeyUp);
    window.addEventListener("blur", onWindowBlur);
    document.addEventListener("visibilitychange", onVisibilityChange);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("keyup", onKeyUp);
      window.removeEventListener("blur", onWindowBlur);
      document.removeEventListener("visibilitychange", onVisibilityChange);
      pushToTalkLatchedRef.current = false;
    };
  }, [applyHotkeyAction, hotkeys.config.pushToTalk, hotkeys.config.toggle, tauriAvailable]);

  const activeState: VoiceWaveHudState = useMemo(() => snapshot.state, [snapshot.state]);
  const isPro = useMemo(() => entitlement.isPro, [entitlement.isPro]);
  const isOwnerOverride = useMemo(
    () => entitlement.isOwnerOverride,
    [entitlement.isOwnerOverride]
  );
  const proRequiredFeature = useMemo(() => parseProRequiredFeature(error), [error]);
  const micQualityWarning = useMemo<MicQualityWarning | null>(() => {
    const selectedInput = settings.inputDevice;
    if (!selectedInput || !isLikelyLowQualityMic(selectedInput)) {
      return null;
    }

    const suggestedInput = findRecommendedInputDevice(inputDevices, selectedInput);
    const thresholdNeedsReset = settings.vadThreshold > RECOMMENDED_VAD_THRESHOLD + 0.004;
    const tuningMessage = thresholdNeedsReset
      ? ` VAD is also set high (${settings.vadThreshold.toFixed(3)}), which can suppress words.`
      : "";

    return {
      currentDevice: selectedInput,
      recommendedDevice: suggestedInput,
      message:
        "Selected microphone appears to be a headset/hands-free profile, which often hurts transcript quality." +
        tuningMessage
    };
  }, [inputDevices, settings.inputDevice, settings.vadThreshold]);

  return {
    snapshot,
    settings,
    hotkeys,
    permissions,
    inputDevices,
    audioQualityReport,
    lastLatency,
    diagnosticsStatus,
    lastDiagnosticsExport,
    entitlement,
    isPro,
    isOwnerOverride,
    proRequiredFeature,
    lastHistoryExport,
    recentInsertions,
    lastInsertion,
    lastHotkeyEvent,
    modelCatalog,
    installedModels,
    modelStatuses,
    benchmarkResults,
    modelRecommendation,
    modelSpeeds,
    historyPolicy,
    sessionHistory,
    dictionaryQueue,
    dictionaryTerms,
    voiceSnippets,
    tauriAvailable,
    activeState,
    micQualityWarning,
    isBusy,
    error,
    runDictation,
    stopDictation,
    setInputDevice,
    switchToRecommendedInput,
    setVadThreshold,
    resetVadThreshold,
    setMaxUtteranceMs,
    setReleaseTailMs,
    setDecodeMode,
    setMicVolumeGuard,
    setTheme,
    setFormatProfile,
    setDomainPacks,
    setAppProfiles,
    setCodeModeSettings,
    setDictationProfile,
    setProPostProcessingEnabled,
    setDiagnosticsOptIn,
    setAnonymousUsageOptIn,
    exportDiagnosticsBundle,
    recommendedVadThreshold: RECOMMENDED_VAD_THRESHOLD,
    setPreferClipboardFallback,
    setSpokenEditCommands,
    completeOnboarding,
    restartOnboarding,
    setLlmPolishEnabled,
    polishModelProgress,
    updateHotkeys,
    requestMicAccess,
    runAudioQualityDiagnostic,
    undoInsertion,
    insertFinalTranscript,
    installModel,
    cancelModelInstall,
    pauseModelInstall,
    resumeModelInstall,
    makeModelActive,
    runBenchmarkAndRecommend,
    updateRetentionPolicy,
    searchHistory,
    addSessionTag,
    setSessionStarred,
    exportHistoryPreset,
    pruneHistory,
    clearSessionHistory,
    approveDictionaryQueueEntry,
    rejectDictionaryQueueEntry,
    deleteDictionaryTerm,
    addDictionaryTerm,
    exportDictionary,
    importDictionary,
    refreshDictionary,
    syncDictionaryWithCloud,
    addVoiceSnippet,
    updateVoiceSnippet,
    deleteVoiceSnippet,
    refreshVoiceSnippets,
    syncVoiceSnippetsWithCloud,
    canUseFeature,
    startProCheckout: startProCheckoutAction,
    refreshEntitlement,
    restorePurchase: restoreProPurchase,
    openBillingPortal: openProBillingPortal,
    setOwnerOverride,
    refreshPhase3Data,
    refreshInputDevices
  };
}
