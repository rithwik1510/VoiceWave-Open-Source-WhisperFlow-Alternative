export type VoiceWaveHudState =
  | "idle"
  | "listening"
  | "transcribing"
  | "inserted"
  | "error";

export type DictationMode = "microphone" | "fixture";
export type DecodeMode = "balanced" | "fast" | "quality";
export type FormatProfile = "default" | "academic" | "technical" | "concise" | "code-doc";
export type DomainPackId = "coding" | "student" | "productivity";
export type AppTargetClass = "editor" | "browser" | "collab" | "desktop";
export type CodeCasingStyle = "preserve" | "camelCase" | "snakeCase" | "pascalCase";

export interface AppProfileBehavior {
  punctuationAggressiveness: number;
  sentenceCompactness: number;
  autoListFormatting: boolean;
}

export interface AppProfileOverrides {
  activeTarget: AppTargetClass;
  editor: AppProfileBehavior;
  browser: AppProfileBehavior;
  collab: AppProfileBehavior;
  desktop: AppProfileBehavior;
}

export interface CodeModeSettings {
  enabled: boolean;
  spokenSymbols: boolean;
  preferredCasing: CodeCasingStyle;
  wrapInFencedBlock: boolean;
}

export type MicVolumeGuardMode = "off" | "warn" | "autoRestore";

export interface VoiceWaveSettings {
  inputDevice: string | null;
  activeModel: string;
  showFloatingHud: boolean;
  vadThreshold: number;
  maxUtteranceMs: number;
  releaseTailMs: number;
  decodeMode: DecodeMode;
  diagnosticsOptIn: boolean;
  toggleHotkey: string;
  pushToTalkHotkey: string;
  preferClipboardFallback: boolean;
  formatProfile: FormatProfile;
  activeDomainPacks: DomainPackId[];
  appProfileOverrides: AppProfileOverrides;
  codeMode: CodeModeSettings;
  proPostProcessingEnabled: boolean;
  /** Always-on spoken structural commands ("new line", "new paragraph",
   * "bullet point") independent of the format profile. Defaults to true. */
  spokenEditCommands: boolean;
  micVolumeGuard: MicVolumeGuardMode;
  /** Opt-in one-tap "Add to dictionary?" pill suggestion. Off by default;
   * optional here since the backend supplies a default when absent. */
  pillActionSuggestions?: boolean;
  /** Experimental on-device AI polish (plan 005). Off by default; optional
   * here since the backend supplies a `false` default when absent. */
  llmPolishEnabled?: boolean;
  /** True once the first-run onboarding flow finished (or was skipped). */
  onboardingCompleted?: boolean;
}

export interface VoiceWaveSnapshot {
  state: VoiceWaveHudState;
  lastPartial: string | null;
  lastFinal: string | null;
  activeModel: string;
}

/** A typed one-tap action carried by a pill notice. The frontend switches on
 * `kind` to render the button and dispatch the handler. */
export interface PillAction {
  kind: "copyTranscript" | "addDictionaryTerm";
  /** Button label, e.g. "Copy" or 'Add "foo"'. */
  label: string;
  /** Action data (e.g. the term to add). `copyTranscript` reads the rescue
   * transcript from `transcript`, so its `value` is null. */
  value: string | null;
}

/** Payload for `voicewave://pill-notice` — the pill's Dynamic Island surface. */
export interface PillNoticePayload {
  id: number;
  severity: "info" | "warning" | "error";
  title: string;
  detail: string | null;
  durationMs: number;
  /** Rescue notices carry the dictated text so failed insertions never lose words. */
  transcript: string | null;
  /** A typed action makes the pill interactive and renders a button. */
  action: PillAction | null;
}

export interface TranscriptEvent {
  text: string;
  isFinal: boolean;
  elapsedMs: number;
}

export interface VoiceWaveStateEvent {
  state: VoiceWaveHudState;
  message?: string | null;
}

export interface LatencyBreakdownEvent {
  sessionId: number;
  captureMs: number;
  releaseToTranscribingMs: number;
  watchdogRecovered: boolean;
  segmentsCaptured: number;
  releaseStopDetectedAtUtcMs: number;
  modelInitMs: number;
  audioConditionMs: number;
  decodeComputeMs: number;
  runtimeCacheHit: boolean;
  backendRequested: string;
  backendUsed: string;
  backendFallback: boolean;
  holdToFirstDraftMs: number;
  incrementalDecodeMs: number;
  releaseFinalizeMs: number;
  incrementalWindowsDecoded: number;
  finalizeTailAudioMs: number;
  asrIntegrityPercent: number;
  asrRawWordCount: number;
  asrFinalWordCount: number;
  decodeMs: number;
  postMs: number;
  insertMs: number;
  totalMs: number;
  releaseToInsertedMs: number;
  audioDurationMs: number;
  modelId: string;
  decodeMode: DecodeMode;
  decodePolicyModeSelected: DecodeMode;
  decodePolicyReason: string;
  fwLowCoherence: boolean;
  fwRetryUsed: boolean;
  fwLiteralRetryUsed: boolean;
  audioPipelineVersion?: string;
  fwAvgLogprob?: number | null;
  fwNoSpeechProb?: number | null;
  fwCompressionRatio?: number | null;
  fwShadowCandidateVersion?: string | null;
  fwShadowQualityDelta?: number | null;
  fwShadowCandidateAvgLogprob?: number | null;
  fwShadowCandidateNoSpeechProb?: number | null;
  fwShadowCandidateRetryUsed?: boolean | null;
  fwShadowCandidateLowCoherence?: boolean | null;
  fwShadowCandidateDecodeComputeMs?: number | null;
  fwShadowCandidateWon?: boolean | null;
  correctionCandidatesCount?: number;
  insertionMethod: InsertionMethod | string;
  insertionTargetClass: string;
}

export interface DiagnosticsStatus {
  optIn: boolean;
  recordCount: number;
  lastExportPath: string | null;
  lastExportedAtUtcMs: number | null;
  watchdogRecoveryCount: number;
}

export interface DiagnosticsExportResult {
  filePath: string;
  exportedAtUtcMs: number;
  recordCount: number;
  redactionSummary: string;
}

export interface HotkeyConfig {
  toggle: string;
  pushToTalk: string;
}

export interface HotkeySnapshot {
  config: HotkeyConfig;
  conflicts: string[];
  registrationSupported: boolean;
  registrationError?: string | null;
}

export type HotkeyAction = "toggleDictation" | "pushToTalk";
export type HotkeyPhase = "pressed" | "released" | "triggered";

export interface HotkeyEvent {
  action: HotkeyAction;
  phase: HotkeyPhase;
}

export type MicrophonePermission = "granted" | "denied" | "unknown";
export type InsertionCapability = "available" | "restricted";

export interface PermissionSnapshot {
  microphone: MicrophonePermission;
  insertionCapability: InsertionCapability;
  message?: string | null;
}

export interface MicLevelEvent {
  level: number;
  error?: string | null;
}

/** Aggregate dictation statistics (hero tier). All values computed on-device
 * from anonymous per-day rollups — never transcript text. */
export interface StatsSummary {
  todayWords: number;
  weekWords: number;
  monthWords: number;
  prevMonthWords: number;
  allTimeWords: number;
  allTimeDictations: number;
  speakingMs: number;
  averageWpm: number;
  bestDictationWpm: number;
  timeSavedMsAllTime: number;
  timeSavedMsMonth: number;
  typingBaselineWpm: number;
  activeDays: number;
}

export type AudioQualityBand = "good" | "fair" | "poor";

export interface AudioQualityReport {
  sampleRate: number;
  segmentCount: number;
  totalSamples: number;
  durationMs: number;
  rms: number;
  peak: number;
  clippingRatio: number;
  lowEnergyFrameRatio: number;
  estimatedSnrDb: number;
  quality: AudioQualityBand;
  issues: string[];
  recommendations: string[];
}

export type InsertionMethod = "direct" | "clipboardPaste" | "clipboardOnly" | "historyFallback";

export interface InsertTextRequest {
  text: string;
  targetApp?: string | null;
  preferClipboard?: boolean;
}

export interface InsertResult {
  success: boolean;
  method: InsertionMethod;
  message?: string | null;
  targetApp?: string | null;
  transactionId: string;
  undoAvailable: boolean;
}

export interface UndoResult {
  success: boolean;
  message?: string | null;
  transactionId?: string | null;
}

export interface RecentInsertion {
  transactionId: string;
  targetApp?: string | null;
  preview: string;
  method: InsertionMethod;
  success: boolean;
  timestampUtcMs: number;
  message?: string | null;
}

export interface ModelCatalogItem {
  modelId: string;
  displayName: string;
  version: string;
  format: string;
  sizeBytes: number;
  sha256: string;
  license: string;
  downloadUrl: string;
  signature: string;
}

export interface InstalledModel {
  modelId: string;
  version: string;
  format: string;
  sizeBytes: number;
  filePath: string;
  sha256: string;
  installedAtUtcMs: number;
  checksumVerified: boolean;
}

export type ModelStatusState =
  | "idle"
  | "downloading"
  | "paused"
  | "installed"
  | "failed"
  | "cancelled";

export interface ModelStatus {
  modelId: string;
  state: ModelStatusState;
  progress: number;
  active: boolean;
  installed: boolean;
  message?: string | null;
  installedModel?: InstalledModel | null;
  downloadedBytes?: number | null;
  totalBytes?: number | null;
  resumable: boolean;
}

export interface ModelEvent {
  modelId: string;
  state: ModelStatusState;
  progress: number;
  message?: string | null;
  downloadedBytes?: number | null;
  totalBytes?: number | null;
}

export interface ModelDownloadRequest {
  modelId: string;
}

export interface BenchmarkRequest {
  modelIds?: string[] | null;
  runsPerModel?: number | null;
  partialDelayMs?: number | null;
}

export interface BenchmarkRow {
  modelId: string;
  runs: number;
  p50LatencyMs: number;
  p95LatencyMs: number;
  averageRtf: number;
  observedSampleCount?: number;
  observedSuccessRatePercent?: number;
  observedP95ReleaseToFinalMs?: number;
  observedP95ReleaseToTranscribingMs?: number;
  observedWatchdogRecoveryRatePercent?: number;
}

export interface BenchmarkRun {
  startedAtUtcMs: number;
  completedAtUtcMs: number;
  rows: BenchmarkRow[];
}

export interface RecommendationConstraints {
  maxP95LatencyMs?: number | null;
  maxRtf?: number | null;
}

export interface ModelRecommendation {
  modelId: string;
  reason: string;
  p95LatencyMs: number;
  averageRtf: number;
  meetsLatencyGate: boolean;
  meetsRtfGate: boolean;
  observedSampleCount?: number;
  observedSuccessRatePercent?: number;
}

export type RetentionPolicy = "off" | "days7" | "days30" | "forever";

export interface SessionHistoryQuery {
  limit?: number | null;
  includeFailed?: boolean | null;
  searchQuery?: string | null;
  tags?: string[] | null;
  starred?: boolean | null;
}

export interface SessionHistoryRecord {
  recordId: string;
  timestampUtcMs: number;
  preview: string;
  /** Full transcript text. Optional: records persisted before v0.5.2 only
   * carry the 140-char preview. */
  text?: string;
  method?: InsertionMethod | null;
  success: boolean;
  source: string;
  message?: string | null;
  tags: string[];
  starred: boolean;
}

export type HistoryExportPreset = "plain" | "markdownNotes" | "studySummary";

export interface HistoryExportResult {
  preset: HistoryExportPreset;
  recordCount: number;
  content: string;
}

export interface DictionaryQueueItem {
  entryId: string;
  term: string;
  sourcePreview: string;
  createdAtUtcMs: number;
}

export interface DictionaryTerm {
  termId: string;
  term: string;
  source: string;
  createdAtUtcMs: number;
}

export interface DictionaryExport {
  version: number;
  exportedAtUtcMs: number;
  terms: DictionaryTerm[];
}

export interface DictionaryImportSummary {
  added: number;
  skipped: number;
  totalInFile: number;
}

export type EntitlementTier = "free" | "pro";
export type EntitlementStatus =
  | "free"
  | "pro_active"
  | "grace"
  | "expired"
  | "owner_override";

export interface BillingPlanDisplay {
  basePriceUsdMonthly: number;
  launchPriceUsdMonthly: number;
  launchMonths: number;
  displayBasePrice: string;
  displayLaunchPrice: string;
  offerCopy: string;
}

export interface EntitlementSnapshot {
  tier: EntitlementTier;
  status: EntitlementStatus;
  isPro: boolean;
  isOwnerOverride: boolean;
  expiresAtUtcMs?: number | null;
  graceUntilUtcMs?: number | null;
  lastRefreshedAtUtcMs: number;
  plan: BillingPlanDisplay;
  message?: string | null;
}

export interface CheckoutLaunchResult {
  url: string;
  launched: boolean;
  message?: string | null;
}

export interface GateErrorPayload {
  code: "PRO_REQUIRED";
  featureId: ProFeatureId;
  message: string;
}

export type ProFeatureId =
  | "format_profile"
  | "domain_packs"
  | "app_profiles"
  | "code_mode"
  | "post_processing"
  | "advanced_history";

export interface ProCatalogItem {
  featureId: ProFeatureId;
  title: string;
  summary: string;
}
