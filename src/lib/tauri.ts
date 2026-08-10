import type { Event } from "@tauri-apps/api/event";
import type {
  AudioQualityReport,
  BenchmarkRequest,
  BenchmarkRun,
  CheckoutLaunchResult,
  CodeModeSettings,
  DomainPackId,
  DiagnosticsExportResult,
  DiagnosticsStatus,
  DictionaryExport,
  DictionaryImportSummary,
  DictionaryQueueItem,
  DictionaryReconcileResult,
  DictionarySyncRecord,
  DictionaryTerm,
  DictationMode,
  AppProfileOverrides,
  EntitlementSnapshot,
  FormatProfile,
  HotkeyAction,
  HotkeyConfig,
  HotkeyEvent,
  HotkeyPhase,
  HotkeySnapshot,
  InstalledModel,
  InsertResult,
  InsertTextRequest,
  LatencyBreakdownEvent,
  ModelCatalogItem,
  ModelDownloadRequest,
  ModelEvent,
  ModelRecommendation,
  ModelStatus,
  MicLevelEvent,
  PillNoticePayload,
  PolishProfile,
  PermissionSnapshot,
  RecentInsertion,
  RecommendationConstraints,
  RetentionPolicy,
  HistoryExportPreset,
  HistoryExportResult,
  SessionHistoryQuery,
  SessionHistoryRecord,
  StatsSummary,
  TranscriptEvent,
  UndoResult,
  VoiceWaveSettings,
  VoiceSnippet,
  VoiceSnippetReconcileResult,
  VoiceSnippetSyncRecord,
  VoiceWaveSnapshot,
  VoiceWaveStateEvent
} from "../types/voicewave";

const isTauriRuntime = () =>
  typeof window !== "undefined" &&
  (
    "__TAURI_INTERNALS__" in window ||
    "__TAURI__" in window ||
    "__TAURI_METADATA__" in window ||
    "__TAURI_IPC__" in window ||
    window.location.protocol === "tauri:" ||
    window.location.protocol === "asset:" ||
    (typeof navigator !== "undefined" && /tauri/i.test(navigator.userAgent))
  );

type UnlistenFn = () => void;

export async function invokeVoicewave<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauriRuntime()) {
    throw new Error("Tauri runtime is not available.");
  }

  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(command, args);
}

export interface PolishModelStatus {
  present: boolean;
  downloading: boolean;
}

export interface PolishModelProgress {
  downloaded: number;
  total: number;
  done: boolean;
  error: string | null;
}

/** Whether the on-device AI-polish model is downloaded and/or downloading. */
export async function getPolishModelStatus(): Promise<PolishModelStatus> {
  return invokeVoicewave<PolishModelStatus>("polish_model_status");
}

/** Kick off the (idempotent, single-flighted) ~1 GB polish-model download. */
export async function downloadPolishModel(): Promise<void> {
  await invokeVoicewave<void>("download_polish_model");
}

export async function listenVoicewavePolishModelProgress(
  callback: (payload: PolishModelProgress) => void
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) {
    return () => undefined;
  }
  const { listen } = await import("@tauri-apps/api/event");
  return listen("voicewave://polish-model-progress", (event: Event<PolishModelProgress>) =>
    callback(event.payload)
  );
}

export async function listenVoicewaveState(
  callback: (payload: VoiceWaveStateEvent) => void
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) {
    return () => undefined;
  }
  const { listen } = await import("@tauri-apps/api/event");
  return listen("voicewave://state", (event: Event<VoiceWaveStateEvent>) => callback(event.payload));
}

export async function listenVoicewaveTranscript(
  callback: (payload: TranscriptEvent) => void
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) {
    return () => undefined;
  }
  const { listen } = await import("@tauri-apps/api/event");
  return listen("voicewave://transcript", (event: Event<TranscriptEvent>) => callback(event.payload));
}

export async function listenVoicewaveInsertion(
  callback: (payload: InsertResult) => void
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) {
    return () => undefined;
  }
  const { listen } = await import("@tauri-apps/api/event");
  return listen("voicewave://insertion", (event: Event<InsertResult>) => callback(event.payload));
}

export async function listenVoicewavePermission(
  callback: (payload: PermissionSnapshot) => void
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) {
    return () => undefined;
  }
  const { listen } = await import("@tauri-apps/api/event");
  return listen("voicewave://permission", (event: Event<PermissionSnapshot>) => callback(event.payload));
}

export async function listenVoicewaveHotkey(
  callback: (payload: HotkeyEvent) => void
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) {
    return () => undefined;
  }
  const { listen } = await import("@tauri-apps/api/event");
  return listen("voicewave://hotkey", (event: Event<HotkeyEvent>) => callback(event.payload));
}

export async function listenVoicewaveModel(
  callback: (payload: ModelEvent) => void
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) {
    return () => undefined;
  }
  const { listen } = await import("@tauri-apps/api/event");
  return listen("voicewave://model", (event: Event<ModelEvent>) => callback(event.payload));
}

export async function listenVoicewaveMicLevel(
  callback: (payload: MicLevelEvent) => void
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) {
    return () => undefined;
  }
  const { listen } = await import("@tauri-apps/api/event");
  return listen("voicewave://mic-level", (event: Event<MicLevelEvent>) => callback(event.payload));
}

export async function listenVoicewavePillNotice(
  callback: (payload: PillNoticePayload) => void
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) {
    return () => undefined;
  }
  const { listen } = await import("@tauri-apps/api/event");
  return listen("voicewave://pill-notice", (event: Event<PillNoticePayload>) =>
    callback(event.payload)
  );
}

export async function listenVoicewaveAudioQuality(
  callback: (payload: AudioQualityReport) => void
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) {
    return () => undefined;
  }
  const { listen } = await import("@tauri-apps/api/event");
  return listen("voicewave://audio-quality", (event: Event<AudioQualityReport>) => callback(event.payload));
}

export async function listenVoicewaveLatency(
  callback: (payload: LatencyBreakdownEvent) => void
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) {
    return () => undefined;
  }
  const { listen } = await import("@tauri-apps/api/event");
  return listen("voicewave://latency", (event: Event<LatencyBreakdownEvent>) => callback(event.payload));
}

/** Fired by the backend after a dictation's history record is persisted, so
 * the UI can refresh its history list without polling. */
export async function listenVoicewaveHistoryUpdated(callback: () => void): Promise<UnlistenFn> {
  if (!isTauriRuntime()) {
    return () => undefined;
  }
  const { listen } = await import("@tauri-apps/api/event");
  return listen("voicewave://history-updated", () => callback());
}

export function canUseTauri(): boolean {
  return isTauriRuntime();
}

export async function loadSnapshot(): Promise<VoiceWaveSnapshot> {
  return invokeVoicewave<VoiceWaveSnapshot>("get_voicewave_snapshot");
}

export async function loadSettings(): Promise<VoiceWaveSettings> {
  return invokeVoicewave<VoiceWaveSettings>("load_settings");
}

export async function getEntitlementSnapshot(): Promise<EntitlementSnapshot> {
  return invokeVoicewave<EntitlementSnapshot>("get_entitlement_snapshot");
}

export async function startProCheckout(): Promise<CheckoutLaunchResult> {
  return invokeVoicewave<CheckoutLaunchResult>("start_pro_checkout");
}

export async function refreshEntitlement(): Promise<EntitlementSnapshot> {
  return invokeVoicewave<EntitlementSnapshot>("refresh_entitlement");
}

export async function restorePurchase(): Promise<EntitlementSnapshot> {
  return invokeVoicewave<EntitlementSnapshot>("restore_purchase");
}

export async function openBillingPortal(): Promise<CheckoutLaunchResult> {
  return invokeVoicewave<CheckoutLaunchResult>("open_billing_portal");
}

export async function setOwnerDeviceOverride(
  enabled: boolean,
  passphrase: string
): Promise<EntitlementSnapshot> {
  return invokeVoicewave<EntitlementSnapshot>("set_owner_device_override", {
    enabled,
    passphrase
  });
}

export async function updateSettings(settings: VoiceWaveSettings): Promise<VoiceWaveSettings> {
  return invokeVoicewave<VoiceWaveSettings>("update_settings", { settings });
}

export async function setFormatProfile(profile: FormatProfile): Promise<VoiceWaveSettings> {
  return invokeVoicewave<VoiceWaveSettings>("set_format_profile", { profile });
}

export async function setActiveDomainPacks(packs: DomainPackId[]): Promise<VoiceWaveSettings> {
  return invokeVoicewave<VoiceWaveSettings>("set_active_domain_packs", { packs });
}

export async function setAppProfileOverrides(
  overrides: AppProfileOverrides
): Promise<VoiceWaveSettings> {
  return invokeVoicewave<VoiceWaveSettings>("set_app_profile_overrides", { overrides });
}

export async function setCodeModeSettings(
  settings: CodeModeSettings
): Promise<VoiceWaveSettings> {
  return invokeVoicewave<VoiceWaveSettings>("set_code_mode_settings", { settings });
}

/** Selects a polish profile via ONE atomic settings write (plan 010). This is
 * the only sanctioned way to change profiles — the frontend must never
 * reconstruct a profile through the individual field setters above (that path
 * is a deprecated fallback for backends without this command). */
export async function setDictationProfile(profile: PolishProfile): Promise<VoiceWaveSettings> {
  return invokeVoicewave<VoiceWaveSettings>("set_dictation_profile", { profile });
}

export async function setProPostProcessingEnabled(
  enabled: boolean
): Promise<VoiceWaveSettings> {
  return invokeVoicewave<VoiceWaveSettings>("set_pro_post_processing_enabled", { enabled });
}

export async function getDiagnosticsStatus(): Promise<DiagnosticsStatus> {
  return invokeVoicewave<DiagnosticsStatus>("get_diagnostics_status");
}

export async function setDiagnosticsOptIn(enabled: boolean): Promise<DiagnosticsStatus> {
  return invokeVoicewave<DiagnosticsStatus>("set_diagnostics_opt_in", { enabled });
}

export async function exportDiagnosticsBundle(): Promise<DiagnosticsExportResult> {
  return invokeVoicewave<DiagnosticsExportResult>("export_diagnostics_bundle");
}

export async function loadHotkeyConfig(): Promise<HotkeySnapshot> {
  return invokeVoicewave<HotkeySnapshot>("load_hotkey_config");
}

export async function updateHotkeyConfig(config: HotkeyConfig): Promise<HotkeySnapshot> {
  return invokeVoicewave<HotkeySnapshot>("update_hotkey_config", { config });
}

export async function getPermissionSnapshot(): Promise<PermissionSnapshot> {
  return invokeVoicewave<PermissionSnapshot>("get_permission_snapshot");
}

export async function listInputDevices(): Promise<string[]> {
  return invokeVoicewave<string[]>("list_input_devices");
}

export async function requestMicrophoneAccess(): Promise<PermissionSnapshot> {
  return invokeVoicewave<PermissionSnapshot>("request_microphone_access");
}

export async function startMicLevelMonitor(): Promise<void> {
  await invokeVoicewave<void>("start_mic_level_monitor");
}

/** Forward live mic-level frames to the main window (onboarding mic check
 * only — steady state keeps them pill-only for perf). */
export async function setMicLevelForwarding(enabled: boolean): Promise<void> {
  await invokeVoicewave<void>("set_mic_level_forwarding", { enabled });
}

export async function stopMicLevelMonitor(): Promise<void> {
  await invokeVoicewave<void>("stop_mic_level_monitor");
}

export async function runAudioQualityDiagnostic(durationMs?: number): Promise<AudioQualityReport> {
  return invokeVoicewave<AudioQualityReport>("run_audio_quality_diagnostic", {
    durationMs: durationMs ?? null
  });
}

export async function insertText(payload: InsertTextRequest): Promise<InsertResult> {
  return invokeVoicewave<InsertResult>("insert_text", { payload });
}

export async function undoLastInsertion(): Promise<UndoResult> {
  return invokeVoicewave<UndoResult>("undo_last_insertion");
}

export async function getRecentInsertions(limit = 10): Promise<RecentInsertion[]> {
  return invokeVoicewave<RecentInsertion[]>("get_recent_insertions", { limit });
}

export async function startDictation(mode: DictationMode = "microphone"): Promise<void> {
  await invokeVoicewave<void>("start_dictation", { mode });
}

export async function cancelDictation(): Promise<void> {
  await invokeVoicewave<void>("cancel_dictation");
}

export async function stopDictation(): Promise<void> {
  await invokeVoicewave<void>("stop_dictation");
}

export async function showMainWindow(): Promise<void> {
  await invokeVoicewave<void>("show_main_window");
}

export async function setPillReviewMode(reviewMode: boolean): Promise<void> {
  await invokeVoicewave<void>("set_pill_review_mode", { reviewMode });
}

export async function setPillNoticeMode(noticeMode: boolean, interactive = false): Promise<void> {
  await invokeVoicewave<void>("set_pill_notice_mode", { noticeMode, interactive });
}

export async function copyTextToClipboard(text: string): Promise<void> {
  await invokeVoicewave<void>("copy_text_to_clipboard", { text });
}

export async function triggerHotkeyAction(action: HotkeyAction, phase: HotkeyPhase): Promise<void> {
  await invokeVoicewave<void>("trigger_hotkey_action", { action, phase });
}

export async function listModelCatalog(): Promise<ModelCatalogItem[]> {
  return invokeVoicewave<ModelCatalogItem[]>("list_model_catalog");
}

export async function listInstalledModels(): Promise<InstalledModel[]> {
  return invokeVoicewave<InstalledModel[]>("list_installed_models");
}

export async function getModelStatus(modelId: string): Promise<ModelStatus> {
  return invokeVoicewave<ModelStatus>("get_model_status", { modelId });
}

export async function downloadModel(request: ModelDownloadRequest): Promise<ModelStatus> {
  return invokeVoicewave<ModelStatus>("download_model", { request });
}

export async function cancelModelDownload(modelId: string): Promise<ModelStatus> {
  return invokeVoicewave<ModelStatus>("cancel_model_download", { modelId });
}

export async function pauseModelDownload(modelId: string): Promise<ModelStatus> {
  return invokeVoicewave<ModelStatus>("pause_model_download", { modelId });
}

export async function resumeModelDownload(modelId: string): Promise<ModelStatus> {
  return invokeVoicewave<ModelStatus>("resume_model_download", { modelId });
}

export async function setActiveModel(modelId: string): Promise<VoiceWaveSettings> {
  return invokeVoicewave<VoiceWaveSettings>("set_active_model", { modelId });
}

export async function runModelBenchmark(request?: BenchmarkRequest): Promise<BenchmarkRun> {
  return invokeVoicewave<BenchmarkRun>("run_model_benchmark", { request: request ?? null });
}

export async function getBenchmarkResults(): Promise<BenchmarkRun | null> {
  return invokeVoicewave<BenchmarkRun | null>("get_benchmark_results");
}

export async function recommendModel(
  constraints?: RecommendationConstraints
): Promise<ModelRecommendation> {
  return invokeVoicewave<ModelRecommendation>("recommend_model", { constraints: constraints ?? null });
}

export async function getSessionHistory(query?: SessionHistoryQuery): Promise<SessionHistoryRecord[]> {
  return invokeVoicewave<SessionHistoryRecord[]>("get_session_history", { query: query ?? null });
}

export async function searchSessionHistory(
  query: string,
  tags?: string[] | null,
  starred?: boolean | null
): Promise<SessionHistoryRecord[]> {
  return invokeVoicewave<SessionHistoryRecord[]>("search_session_history", {
    query,
    tags: tags ?? null,
    starred: starred ?? null
  });
}

export async function tagSession(recordId: string, tag: string): Promise<SessionHistoryRecord> {
  return invokeVoicewave<SessionHistoryRecord>("tag_session", { recordId, tag });
}

export async function toggleStarSession(
  recordId: string,
  starred: boolean
): Promise<SessionHistoryRecord> {
  return invokeVoicewave<SessionHistoryRecord>("toggle_star_session", { recordId, starred });
}

export async function exportSessionHistoryPreset(
  preset: HistoryExportPreset
): Promise<HistoryExportResult> {
  return invokeVoicewave<HistoryExportResult>("export_session_history_preset", { preset });
}

export async function setHistoryRetention(policy: RetentionPolicy): Promise<RetentionPolicy> {
  return invokeVoicewave<RetentionPolicy>("set_history_retention", { policy });
}

export async function getHistoryRetention(): Promise<RetentionPolicy> {
  return invokeVoicewave<RetentionPolicy>("get_history_retention");
}

export async function getStatsSummary(rangeDays?: number): Promise<StatsSummary> {
  return invokeVoicewave<StatsSummary>(
    "get_stats_summary",
    rangeDays === undefined ? {} : { rangeDays }
  );
}

export async function pruneHistoryNow(): Promise<number> {
  return invokeVoicewave<number>("prune_history_now");
}

export async function clearHistory(): Promise<number> {
  return invokeVoicewave<number>("clear_history");
}

export async function getDictionaryQueue(limit = 50): Promise<DictionaryQueueItem[]> {
  return invokeVoicewave<DictionaryQueueItem[]>("get_dictionary_queue", { limit });
}

export async function approveDictionaryEntry(
  entryId: string,
  normalizedText?: string
): Promise<DictionaryTerm> {
  return invokeVoicewave<DictionaryTerm>("approve_dictionary_entry", {
    entryId,
    normalizedText: normalizedText ?? null
  });
}

export async function rejectDictionaryEntry(entryId: string, reason?: string): Promise<void> {
  await invokeVoicewave<void>("reject_dictionary_entry", { entryId, reason: reason ?? null });
}

export async function getDictionaryTerms(query?: string): Promise<DictionaryTerm[]> {
  return invokeVoicewave<DictionaryTerm[]>("get_dictionary_terms", { query: query ?? null });
}

export async function removeDictionaryTerm(termId: string): Promise<void> {
  await invokeVoicewave<void>("remove_dictionary_term", { termId });
}

export async function addDictionaryTerm(term: string): Promise<DictionaryTerm> {
  return invokeVoicewave<DictionaryTerm>("add_dictionary_term", { term });
}

export async function exportDictionary(): Promise<DictionaryExport> {
  return invokeVoicewave<DictionaryExport>("export_dictionary");
}

export async function importDictionary(payload: string): Promise<DictionaryImportSummary> {
  return invokeVoicewave<DictionaryImportSummary>("import_dictionary", { payload });
}

export async function getDictionarySyncRecords(): Promise<DictionarySyncRecord[]> {
  return invokeVoicewave<DictionarySyncRecord[]>("get_dictionary_sync_records");
}

export async function reconcileDictionaryRecords(
  records: DictionarySyncRecord[]
): Promise<DictionaryReconcileResult> {
  return invokeVoicewave<DictionaryReconcileResult>("reconcile_dictionary_records", { records });
}

export async function listVoiceSnippets(query?: string): Promise<VoiceSnippet[]> {
  return invokeVoicewave<VoiceSnippet[]>("list_voice_snippets", { query: query ?? null });
}

export async function addVoiceSnippet(
  trigger: string,
  expansion: string
): Promise<VoiceSnippet> {
  return invokeVoicewave<VoiceSnippet>("add_voice_snippet", { trigger, expansion });
}

export async function updateVoiceSnippet(
  snippetId: string,
  trigger: string,
  expansion: string
): Promise<VoiceSnippet> {
  return invokeVoicewave<VoiceSnippet>("update_voice_snippet", {
    snippetId,
    trigger,
    expansion
  });
}

export async function removeVoiceSnippet(snippetId: string): Promise<void> {
  await invokeVoicewave<void>("remove_voice_snippet", { snippetId });
}

export async function getVoiceSnippetSyncRecords(): Promise<VoiceSnippetSyncRecord[]> {
  return invokeVoicewave<VoiceSnippetSyncRecord[]>("get_voice_snippet_sync_records");
}

export async function reconcileVoiceSnippetRecords(
  records: VoiceSnippetSyncRecord[]
): Promise<VoiceSnippetReconcileResult> {
  return invokeVoicewave<VoiceSnippetReconcileResult>("reconcile_voice_snippet_records", {
    records
  });
}
