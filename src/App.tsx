import {
  Activity,
  Check,
  ChevronDown,
  CircleHelp,
  Copy,
  Crown,
  Download,
  Keyboard,
  Mic,
  Palette,
  Search,
  SlidersHorizontal,
  Sparkles,
  Star,
  X
} from "lucide-react";
import { type FormEvent, type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import {
  addCloudDictionaryTerm,
  type CloudSentence,
  deleteCloudDictionaryTerm,
  ensureCloudProfile,
  getCloudErrorMessage,
  listCloudDictionaryTerms,
  listRecentCloudSentences,
  requestPasswordResetCloud,
  saveCloudSentence,
  signInCloud,
  signOutCloud,
  signUpCloud,
  subscribeCloudAuth
} from "./lib/cloudSync";
import { firebaseEnabled } from "./lib/firebase";
import { copyTextToClipboard } from "./lib/tauri";
import { Onboarding } from "./components/Onboarding";
import { UpdatePrompt } from "./components/UpdatePrompt";
import { UpdateSection } from "./components/UpdateSection";
import { useVoiceWave } from "./hooks/useVoiceWave";
import { THEMES } from "./prototype/constants";
import { Dashboard } from "./prototype/components/Dashboard";
import { Layout } from "./prototype/components/Layout";
import type { DictationState } from "./prototype/types";
import type {
  AppProfileOverrides,
  CodeModeSettings,
  DomainPackId,
  DictionaryTerm,
  FormatProfile,
  MicVolumeGuardMode,
  RetentionPolicy,
  VoiceWaveSettings
} from "./types/voicewave";

type OverlayPanel = "style" | "settings" | "help" | "profile" | "auth";
type ProToolsMode = "default" | "coding" | "writing" | "study";
type AuthMode = "signin" | "signup";
type SetupModelChoice = "fw-small.en" | "fw-large-v3-turbo";
type SettingsSection = "audio" | "dictation" | "polish" | "diagnostics" | "advanced" | "updates";

const SETTINGS_SECTIONS: Array<{ id: SettingsSection; label: string; icon: typeof Mic }> = [
  { id: "audio", label: "Audio", icon: Mic },
  { id: "dictation", label: "Dictation", icon: Keyboard },
  { id: "polish", label: "AI Polish", icon: Sparkles },
  { id: "diagnostics", label: "Diagnostics", icon: Activity },
  { id: "advanced", label: "Advanced", icon: SlidersHorizontal },
  { id: "updates", label: "Updates", icon: Download }
];

interface DemoProfile {
  name: string;
  email: string;
  workspaceRole: string;
}

function formatDate(ts: number): string {
  return new Date(ts).toLocaleString();
}

function formatBytes(bytes: number): string {
  if (bytes >= 1024 * 1024) {
    return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
  }
  if (bytes >= 1024) {
    return `${(bytes / 1024).toFixed(1)} KB`;
  }
  return `${bytes} B`;
}

function policyLabel(policy: RetentionPolicy): string {
  if (policy === "off") {
    return "Off";
  }
  if (policy === "days7") {
    return "7 Days";
  }
  if (policy === "days30") {
    return "30 Days";
  }
  return "Forever";
}

interface ProToolsPreset {
  formatProfile: FormatProfile;
  domainPacks: DomainPackId[];
  codeMode: CodeModeSettings;
  appProfiles: AppProfileOverrides;
  postProcessingEnabled: boolean;
}

const PRO_TOOLS_MODE_CARDS: Array<{
  id: ProToolsMode;
  title: string;
  description: string;
  highlight: string;
}> = [
  {
    id: "default",
    title: "Default",
    description: "Closest to classic dictation with light cleanup.",
    highlight: "Best for everyday typing without aggressive transforms."
  },
  {
    id: "coding",
    title: "Coding",
    description: "Voice-to-code setup with symbol handling and coding vocabulary.",
    highlight: "Enables Code Mode + coding domain dictionary."
  },
  {
    id: "writing",
    title: "Writing",
    description: "Cleaner prose output for docs, posts, and polished text.",
    highlight: "Uses formal formatting + productivity wording."
  },
  {
    id: "study",
    title: "Study",
    description: "Note-friendly flow for lectures, revision, and summaries.",
    highlight: "Uses concise formatting + student-focused dictionary."
  }
];

const PRO_HIGHLIGHT_CARDS: Array<{
  id: string;
  icon: typeof Sparkles;
  title: string;
  subtitle: string;
}> = [
  {
    id: "output",
    icon: Sparkles,
    title: "Better Output",
    subtitle: "Profiles + polish"
  },
  {
    id: "workflow",
    icon: Crown,
    title: "Workflow Packs",
    subtitle: "Domain + code mode"
  },
  {
    id: "history",
    icon: Search,
    title: "Power History",
    subtitle: "Search, tag, export"
  }
];

function detectProToolsMode(settings: VoiceWaveSettings): ProToolsMode {
  if (
    settings.codeMode.enabled ||
    settings.formatProfile === "code-doc" ||
    settings.activeDomainPacks.includes("coding")
  ) {
    return "coding";
  }

  if (settings.activeDomainPacks.includes("student")) {
    return "study";
  }

  if (
    settings.formatProfile === "academic" ||
    settings.formatProfile === "concise" ||
    settings.activeDomainPacks.includes("productivity")
  ) {
    return "writing";
  }

  return "default";
}

function buildProToolsPreset(mode: ProToolsMode, settings: VoiceWaveSettings): ProToolsPreset {
  switch (mode) {
    case "coding":
      return {
        formatProfile: "code-doc",
        domainPacks: ["coding"],
        codeMode: {
          ...settings.codeMode,
          enabled: true,
          spokenSymbols: true,
          preferredCasing: "camelCase",
          wrapInFencedBlock: false
        },
        appProfiles: {
          ...settings.appProfileOverrides,
          activeTarget: "editor",
          editor: {
            punctuationAggressiveness: 0,
            sentenceCompactness: 0,
            autoListFormatting: false
          }
        },
        postProcessingEnabled: true
      };
    case "writing":
      return {
        formatProfile: "academic",
        domainPacks: ["productivity"],
        codeMode: {
          ...settings.codeMode,
          enabled: false,
          spokenSymbols: true,
          preferredCasing: "preserve",
          wrapInFencedBlock: false
        },
        appProfiles: {
          ...settings.appProfileOverrides,
          activeTarget: "collab",
          collab: {
            punctuationAggressiveness: 2,
            sentenceCompactness: 1,
            autoListFormatting: true
          }
        },
        postProcessingEnabled: true
      };
    case "study":
      return {
        formatProfile: "concise",
        domainPacks: ["student", "productivity"],
        codeMode: {
          ...settings.codeMode,
          enabled: false,
          spokenSymbols: true,
          preferredCasing: "preserve",
          wrapInFencedBlock: false
        },
        appProfiles: {
          ...settings.appProfileOverrides,
          activeTarget: "browser",
          browser: {
            punctuationAggressiveness: 2,
            sentenceCompactness: 2,
            autoListFormatting: true
          }
        },
        postProcessingEnabled: true
      };
    default:
      return {
        formatProfile: "default",
        domainPacks: [],
        codeMode: {
          ...settings.codeMode,
          enabled: false,
          spokenSymbols: true,
          preferredCasing: "preserve",
          wrapInFencedBlock: false
        },
        appProfiles: {
          ...settings.appProfileOverrides,
          activeTarget: "desktop",
          desktop: {
            punctuationAggressiveness: 1,
            sentenceCompactness: 1,
            autoListFormatting: false
          }
        },
        // Filler pruning / stutter collapse is desirable in every mode, not
        // just the specialized presets — raw "um, I I think" output is the
        // main thing that makes transcripts read worse than cloud apps.
        postProcessingEnabled: true
      };
  }
}

interface OverlayModalProps {
  title: string;
  subtitle: string;
  onClose: () => void;
  children: ReactNode;
  maxWidthClassName?: string;
}

function OverlayModal({ title, subtitle, onClose, children, maxWidthClassName = "max-w-3xl" }: OverlayModalProps) {
  return (
    <div
      className="vw-modal-backdrop"
      onMouseDown={(event) => {
        if (event.currentTarget === event.target) {
          onClose();
        }
      }}
    >
      <section className={`vw-modal-card ${maxWidthClassName}`} role="dialog" aria-modal="true" aria-label={title}>
        <header className="vw-modal-header">
          <div>
            <h3 className="vw-section-heading text-xl font-semibold text-[#09090B]">{title}</h3>
            <p className="mt-1 text-sm text-[#71717A]">{subtitle}</p>
          </div>
          <button type="button" className="vw-modal-close" onClick={onClose} aria-label={`Close ${title}`}>
            <X size={16} />
          </button>
        </header>
        <div className="vw-modal-body">{children}</div>
      </section>
    </div>
  );
}

function App() {
  const theme = THEMES.A;
  const [activeNav, setActiveNav] = useState("home");
  const [activeOverlay, setActiveOverlay] = useState<OverlayPanel | null>(null);
  const [settingsSection, setSettingsSection] = useState<SettingsSection>("audio");
  const [historyQuery, setHistoryQuery] = useState("");
  const [historyTag, setHistoryTag] = useState("");
  const [copiedHistoryId, setCopiedHistoryId] = useState<string | null>(null);
  const [dictionaryDraftTerm, setDictionaryDraftTerm] = useState("");
  const [dictionaryPortNotice, setDictionaryPortNotice] = useState<string | null>(null);
  const dictionaryImportInputRef = useRef<HTMLInputElement | null>(null);
  const [dictionaryPendingOpen, setDictionaryPendingOpen] = useState(false);
  const [ownerTapCount, setOwnerTapCount] = useState(0);
  const [ownerPassphrase, setOwnerPassphrase] = useState("");
  const [modeApplyPending, setModeApplyPending] = useState<ProToolsMode | null>(null);
  const [benchmarkPanelOpen, setBenchmarkPanelOpen] = useState(false);
  const [demoProfile, setDemoProfile] = useState<DemoProfile | null>(null);
  const [cloudUserId, setCloudUserId] = useState<string | null>(null);
  const [cloudRecentSentences, setCloudRecentSentences] = useState<CloudSentence[]>([]);
  const [cloudDictionaryTerms, setCloudDictionaryTerms] = useState<DictionaryTerm[]>([]);
  const [authMode, setAuthMode] = useState<AuthMode>("signin");
  const [authName, setAuthName] = useState("");
  const [authEmail, setAuthEmail] = useState("");
  const [authPassword, setAuthPassword] = useState("");
  const [authConfirmPassword, setAuthConfirmPassword] = useState("");
  const [authWorkspaceRole, setAuthWorkspaceRole] = useState("");
  const [authShowPassword, setAuthShowPassword] = useState(false);
  const [authShowConfirmPassword, setAuthShowConfirmPassword] = useState(false);
  const [authRememberMe, setAuthRememberMe] = useState(true);
  const [authPending, setAuthPending] = useState(false);
  const [authError, setAuthError] = useState<string | null>(null);
  const [authNotice, setAuthNotice] = useState<string | null>(null);
  const [cloudSyncError, setCloudSyncError] = useState<string | null>(null);
  const [setupModelChoice, setSetupModelChoice] = useState<SetupModelChoice>("fw-small.en");
  const [setupModelPending, setSetupModelPending] = useState(false);
  const [setupModelError, setSetupModelError] = useState<string | null>(null);
  const {
    activeState,
    approveDictionaryQueueEntry,
    benchmarkResults,
    cancelModelInstall,
    clearSessionHistory,
    diagnosticsStatus,
    deleteDictionaryTerm,
    dictionaryQueue,
    dictionaryTerms,
    entitlement,
    error,
    exportHistoryPreset,
    exportDiagnosticsBundle,
    isOwnerOverride,
    isPro,
    historyPolicy,
    inputDevices,
    installModel,
    installedModels,
    makeModelActive,
    modelCatalog,
    modelRecommendation,
    modelSpeeds,
    modelStatuses,
    lastHistoryExport,
    lastDiagnosticsExport,
    lastLatency,
    permissions,
    proRequiredFeature,
    audioQualityReport,
    micQualityWarning,
    pauseModelInstall,
    pruneHistory,
    refreshPhase3Data,
    refreshInputDevices,
    resumeModelInstall,
    rejectDictionaryQueueEntry,
    requestMicAccess,
    runAudioQualityDiagnostic,
    runBenchmarkAndRecommend,
    runDictation,
    searchHistory,
    sessionHistory,
    setAppProfiles,
    setCodeModeSettings,
    setDiagnosticsOptIn,
    setDomainPacks,
    setFormatProfile,
    setInputDevice,
    setMaxUtteranceMs,
    setOwnerOverride,
    setReleaseTailMs,
    setPreferClipboardFallback,
    setSpokenEditCommands,
    completeOnboarding,
    restartOnboarding,
    setLlmPolishEnabled,
    polishModelProgress,
    setMicVolumeGuard,
    setProPostProcessingEnabled,
    setSessionStarred,
    setVadThreshold,
    addSessionTag,
    addDictionaryTerm,
    exportDictionary,
    importDictionary,
    resetVadThreshold,
    settings,
    switchToRecommendedInput,
    recommendedVadThreshold,
    snapshot,
    stopDictation,
    tauriAvailable,
    updateRetentionPolicy,
    refreshEntitlement
  } = useVoiceWave();

  const status = useMemo<DictationState>(() => activeState, [activeState]);
  const displayError = useMemo(() => {
    if (!error) {
      return null;
    }
    if (proRequiredFeature) {
      return "This feature is included in the release offer. Please retry in a moment.";
    }
    return error;
  }, [error, proRequiredFeature]);
  const isRecording = status === "listening" || status === "transcribing";
  const installedModelSet = useMemo(
    () => new Set(installedModels.map((row) => row.modelId)),
    [installedModels]
  );
  const showOwnerUnlock = ownerTapCount >= 5;
  const pressActiveRef = useRef(false);
  const modeApplyInFlightRef = useRef(false);
  const lastCloudSentenceRef = useRef<string | null>(null);
  const activeProToolsMode = useMemo(() => detectProToolsMode(settings), [settings]);
  const displayedProToolsMode = modeApplyPending ?? activeProToolsMode;
  const proStatusLabel = isOwnerOverride ? "Owner Pro (Device Override)" : "Release Offer Active";
  const releaseOfferHeadline = "Pro is unlocked for every workspace during this initial release.";
  const releaseOfferLine = entitlement.plan.offerCopy || "Initial release offer: Pro is included for everyone.";
  const releaseOfferStateLine = useMemo(() => {
    if (isOwnerOverride) {
      return "Owner override is enabled on this machine for internal access.";
    }
    return "Release offer is active. No subscription purchase is required right now.";
  }, [isOwnerOverride]);
  const isDemoAuthenticated = Boolean(demoProfile);
  const profileDisplayName = demoProfile?.name ?? "Workspace";
  const profileStatusLabel = demoProfile
    ? `${isPro ? "Pro" : "Free"} workspace${cloudUserId ? " (cloud)" : ""}`
    : "Guest mode";
  const recentSentences = useMemo(
    () =>
      cloudUserId
        ? cloudRecentSentences
        : [...sessionHistory]
            .sort((left, right) => right.timestampUtcMs - left.timestampUtcMs)
            .slice(0, 5)
            .map((row) => ({
              id: row.recordId,
              text: row.preview,
              createdAtUtcMs: row.timestampUtcMs
            })),
    [cloudRecentSentences, cloudUserId, sessionHistory]
  );
  const activeDictionaryTerms = cloudUserId ? cloudDictionaryTerms : dictionaryTerms;
  const hasInstalledModel = installedModels.length > 0;
  const setupCatalog = useMemo(
    () => modelCatalog.filter((row) => row.modelId === "fw-small.en" || row.modelId === "fw-large-v3-turbo"),
    [modelCatalog]
  );
  const showOnboarding = tauriAvailable && !settings.onboardingCompleted;
  // The plain model gate stays as the fallback for users who skipped
  // onboarding without installing a model; it hides while the flow is up.
  const showModelSetupGate =
    tauriAvailable && !hasInstalledModel && setupCatalog.length > 0 && !showOnboarding;
  const selectedSetupCatalogRow = setupCatalog.find((row) => row.modelId === setupModelChoice) ?? null;
  const selectedSetupStatus = modelStatuses[setupModelChoice] ?? null;

  useEffect(() => {
    if (!showModelSetupGate) {
      return;
    }
    setSetupModelChoice("fw-small.en");
  }, [showModelSetupGate]);

  useEffect(() => {
    if (proRequiredFeature) {
      setActiveNav("pro");
    }
  }, [proRequiredFeature]);

  useEffect(() => {
    if (!isPro && activeNav === "pro-tools") {
      setActiveNav("pro");
    }
  }, [activeNav, isPro]);

  useEffect(() => {
    if (!firebaseEnabled) {
      return;
    }

    const unsubscribe = subscribeCloudAuth((user) => {
      if (!user) {
        setDemoProfile(null);
        setCloudUserId(null);
        setCloudRecentSentences([]);
        setCloudDictionaryTerms([]);
        lastCloudSentenceRef.current = null;
        return;
      }

      void (async () => {
        try {
          const [profile, recent, cloudTerms] = await Promise.all([
            ensureCloudProfile(user, "Personal Workspace"),
            listRecentCloudSentences(user.uid),
            listCloudDictionaryTerms(user.uid)
          ]);
          setDemoProfile(profile);
          setCloudUserId(user.uid);
          setCloudRecentSentences(recent);
          setCloudDictionaryTerms(cloudTerms);
          setCloudSyncError(null);
        } catch (cloudErr) {
          setCloudSyncError(getCloudErrorMessage(cloudErr));
        }
      })();
    });

    return unsubscribe;
  }, []);

  useEffect(() => {
    if (!cloudUserId || !firebaseEnabled) {
      return;
    }
    const finalText = snapshot.lastFinal?.trim();
    if (!finalText) {
      return;
    }
    if (finalText === lastCloudSentenceRef.current) {
      return;
    }
    lastCloudSentenceRef.current = finalText;

    void (async () => {
      try {
        const recent = await saveCloudSentence(cloudUserId, finalText);
        setCloudRecentSentences(recent);
        setCloudSyncError(null);
      } catch (cloudErr) {
        setCloudSyncError(getCloudErrorMessage(cloudErr));
      }
    })();
  }, [cloudUserId, snapshot.lastFinal]);

  const isOverlayNav = (value: string): value is OverlayPanel =>
    value === "style" || value === "settings" || value === "help" || value === "profile" || value === "auth";

  const closeOverlay = () => {
    setActiveOverlay(null);
    setSettingsSection("audio");
  };

  const openOverlay = (panel: OverlayPanel) => {
    pressActiveRef.current = false;
    if (panel !== "settings") {
      setSettingsSection("audio");
    }
    setActiveOverlay(panel);
  };

  const handlePressStart = () => {
    if (isRecording) {
      return;
    }
    pressActiveRef.current = true;
    void runDictation(tauriAvailable ? "microphone" : "fixture");
  };

  const handlePressEnd = () => {
    if (!pressActiveRef.current) {
      return;
    }
    pressActiveRef.current = false;
    void stopDictation();
  };

  const handleNavChange = (nextNav: string) => {
    if (isOverlayNav(nextNav)) {
      openOverlay(nextNav);
      return;
    }

    if (nextNav === "pro-tools" && !isPro) {
      setActiveNav("pro");
      return;
    }

    if (nextNav === activeNav) {
      return;
    }
    // Prevent stale press-and-hold state from surviving page switches.
    pressActiveRef.current = false;
    closeOverlay();
    setActiveNav(nextNav);
  };

  const handleSetupModelInstall = async () => {
    setSetupModelError(null);
    setSetupModelPending(true);
    try {
      await installModel(setupModelChoice);
      await makeModelActive(setupModelChoice);
      setActiveNav("home");
    } catch (setupErr) {
      if (setupErr instanceof Error && setupErr.message) {
        setSetupModelError(setupErr.message);
      } else {
        setSetupModelError("Model installation failed. Please retry.");
      }
    } finally {
      setSetupModelPending(false);
    }
  };

  const handleAuthSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setAuthError(null);
    setAuthNotice(null);
    const normalizedEmail = authEmail.trim();
    if (!normalizedEmail) {
      setAuthError("Please enter your email.");
      return;
    }
    if (!authPassword.trim()) {
      setAuthError("Please enter your password.");
      return;
    }
    if (authMode === "signup") {
      if (authPassword.length < 6) {
        setAuthError("Password must be at least 6 characters.");
        return;
      }
      if (authPassword !== authConfirmPassword) {
        setAuthError("Password and confirm password do not match.");
        return;
      }
    }

    if (firebaseEnabled) {
      setAuthPending(true);
      try {
        const profile =
          authMode === "signup"
            ? await signUpCloud({
                email: normalizedEmail,
                password: authPassword,
                name: authName,
                workspaceRole: authWorkspaceRole
              })
            : await signInCloud(normalizedEmail, authPassword);
        const [recent, cloudTerms] = await Promise.all([
          listRecentCloudSentences(profile.uid),
          listCloudDictionaryTerms(profile.uid)
        ]);
        setDemoProfile({
          name: profile.name,
          email: profile.email,
          workspaceRole: profile.workspaceRole
        });
        setCloudUserId(profile.uid);
        setCloudRecentSentences(recent);
        setCloudDictionaryTerms(cloudTerms);
        setAuthPassword("");
        setAuthConfirmPassword("");
        setActiveOverlay("profile");
      } catch (cloudErr) {
        setAuthError(getCloudErrorMessage(cloudErr));
      } finally {
        setAuthPending(false);
      }
      return;
    }

    const derivedName =
      authMode === "signup"
        ? authName.trim() || normalizedEmail.split("@")[0] || "VoiceWave User"
        : demoProfile?.name || normalizedEmail.split("@")[0] || "VoiceWave User";
    setDemoProfile({
      name: derivedName,
      email: normalizedEmail,
      workspaceRole: authWorkspaceRole.trim() || "Personal Workspace"
    });
    setCloudUserId(null);
    setCloudRecentSentences([]);
    setCloudDictionaryTerms([]);
    setAuthPassword("");
    setAuthConfirmPassword("");
    setActiveOverlay("profile");
  };

  const continueAsGuest = () => {
    setAuthError(null);
    setAuthNotice(null);
    closeOverlay();
  };

  const openAuthOverlay = (mode: AuthMode) => {
    setAuthMode(mode);
    setAuthError(null);
    setAuthNotice(null);
    setAuthShowPassword(false);
    setAuthShowConfirmPassword(false);
    openOverlay("auth");
  };

  const handleSignOut = async () => {
    setAuthError(null);
    setAuthNotice(null);
    if (firebaseEnabled && cloudUserId) {
      setAuthPending(true);
      try {
        await signOutCloud();
        setDemoProfile(null);
        setCloudUserId(null);
        setCloudRecentSentences([]);
        setCloudDictionaryTerms([]);
        openAuthOverlay("signin");
      } catch (cloudErr) {
        setAuthError(getCloudErrorMessage(cloudErr));
      } finally {
        setAuthPending(false);
      }
      return;
    }

    setDemoProfile(null);
    setCloudUserId(null);
    setCloudRecentSentences([]);
    setCloudDictionaryTerms([]);
    openAuthOverlay("signin");
  };

  const handleForgotPassword = async () => {
    setAuthError(null);
    setAuthNotice(null);
    const normalizedEmail = authEmail.trim();
    if (!normalizedEmail) {
      setAuthError("Enter your email, then tap Forgot Password.");
      return;
    }
    if (!firebaseEnabled) {
      setAuthError("Password reset requires Firebase cloud auth to be enabled.");
      return;
    }

    setAuthPending(true);
    try {
      await requestPasswordResetCloud(normalizedEmail);
      setAuthNotice("Password reset email sent. Check your inbox.");
    } catch (cloudErr) {
      setAuthError(getCloudErrorMessage(cloudErr));
    } finally {
      setAuthPending(false);
    }
  };

  const applyProToolsMode = async (mode: ProToolsMode) => {
    if (!isPro) {
      setActiveNav("pro");
      return;
    }
    if (modeApplyInFlightRef.current || modeApplyPending) {
      return;
    }
    if (mode === activeProToolsMode) {
      return;
    }

    const preset = buildProToolsPreset(mode, settings);
    modeApplyInFlightRef.current = true;
    setModeApplyPending(mode);
    try {
      await setFormatProfile(preset.formatProfile);
      await setDomainPacks(preset.domainPacks);
      await setCodeModeSettings(preset.codeMode);
      await setAppProfiles(preset.appProfiles);
      await setProPostProcessingEnabled(preset.postProcessingEnabled);
    } catch (err) {
      console.error("Failed to apply Pro Tools mode:", err);
    } finally {
      modeApplyInFlightRef.current = false;
      setModeApplyPending(null);
    }
  };

  useEffect(() => {
    if (!activeOverlay) {
      return;
    }

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        closeOverlay();
      }
    };

    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    window.addEventListener("keydown", onKeyDown);
    return () => {
      document.body.style.overflow = previousOverflow;
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [activeOverlay]);

  useEffect(() => {
    if (activeNav === "pro") {
      void refreshEntitlement();
    }
  }, [activeNav, refreshEntitlement]);

  const retentionOptions: RetentionPolicy[] = ["off", "days7", "days30", "forever"];
  const domainPackOptions: DomainPackId[] = ["coding", "student", "productivity"];
  const sortedDictionaryTerms = useMemo(
    () => [...activeDictionaryTerms].sort((left, right) => right.createdAtUtcMs - left.createdAtUtcMs),
    [activeDictionaryTerms]
  );
  const sortedDictionaryQueue = useMemo(
    () => [...dictionaryQueue].sort((left, right) => right.createdAtUtcMs - left.createdAtUtcMs),
    [dictionaryQueue]
  );

  const submitDictionaryDraft = () => {
    const normalized = dictionaryDraftTerm.trim();
    if (!normalized) {
      return;
    }
    if (cloudUserId) {
      void (async () => {
        try {
          const nextTerms = await addCloudDictionaryTerm(cloudUserId, normalized, "manual-add");
          setCloudDictionaryTerms(nextTerms);
          setCloudSyncError(null);
        } catch (cloudErr) {
          setCloudSyncError(getCloudErrorMessage(cloudErr));
        }
      })();
      setDictionaryDraftTerm("");
      return;
    }

    void addDictionaryTerm(normalized);
    setDictionaryDraftTerm("");
  };

  const handleDeleteDictionaryTerm = (termId: string) => {
    if (!cloudUserId) {
      void deleteDictionaryTerm(termId);
      return;
    }

    void (async () => {
      try {
        const nextTerms = await deleteCloudDictionaryTerm(cloudUserId, termId);
        setCloudDictionaryTerms(nextTerms);
        setCloudSyncError(null);
      } catch (cloudErr) {
        setCloudSyncError(getCloudErrorMessage(cloudErr));
      }
    })();
  };

  const handleExportDictionary = () => {
    void (async () => {
      try {
        const payload = await exportDictionary();
        const json = JSON.stringify(payload, null, 2);
        const blob = new Blob([json], { type: "application/json" });
        const url = URL.createObjectURL(blob);
        const anchor = document.createElement("a");
        anchor.href = url;
        anchor.download = "voicewave-dictionary.json";
        document.body.appendChild(anchor);
        anchor.click();
        document.body.removeChild(anchor);
        URL.revokeObjectURL(url);
        setDictionaryPortNotice(`Exported ${payload.terms.length} terms.`);
      } catch (exportErr) {
        setDictionaryPortNotice(
          exportErr instanceof Error ? exportErr.message : "Failed to export dictionary"
        );
      }
    })();
  };

  const handleImportDictionaryFile = (event: FormEvent<HTMLInputElement>) => {
    const input = event.currentTarget;
    const file = input.files?.[0];
    if (!file) {
      return;
    }
    void (async () => {
      try {
        const text = await file.text();
        const summary = await importDictionary(text);
        const skippedNote = summary.skipped > 0 ? ` (${summary.skipped} skipped)` : "";
        setDictionaryPortNotice(`Imported ${summary.added} terms${skippedNote}.`);
      } catch (importErr) {
        setDictionaryPortNotice(
          importErr instanceof Error ? importErr.message : "Failed to import dictionary"
        );
      } finally {
        // Reset so re-selecting the same file fires the change event again.
        input.value = "";
      }
    })();
  };

  const handleApproveDictionaryQueueEntry = (entryId: string) => {
    if (!cloudUserId) {
      void approveDictionaryQueueEntry(entryId);
      return;
    }

    const entry = dictionaryQueue.find((row) => row.entryId === entryId);
    if (!entry) {
      return;
    }

    void (async () => {
      try {
        const nextTerms = await addCloudDictionaryTerm(cloudUserId, entry.term, "queue-approval");
        setCloudDictionaryTerms(nextTerms);
        await rejectDictionaryQueueEntry(entryId);
        setCloudSyncError(null);
      } catch (cloudErr) {
        setCloudSyncError(getCloudErrorMessage(cloudErr));
      }
    })();
  };

  return (
    <>
      <UpdatePrompt />
      <Layout
        theme={theme}
        activeNav={activeNav}
        activePopupNav={activeOverlay}
        setActiveNav={handleNavChange}
        isRecording={isRecording}
        isPro={isPro}
        showProTools={isPro}
        profileDisplayName={profileDisplayName}
        profileStatusLabel={profileStatusLabel}
        isProfileAuthenticated={isDemoAuthenticated}
        onUpgradeClick={() => setActiveNav("pro")}
      >
        <div key={activeNav} className={`vw-page-shell ${isPro ? "vw-pro-ui" : ""}`}>
          {activeNav === "home" && (
            <>
              {!tauriAvailable && (
                <div className="mb-6 rounded-2xl border border-[#f3c2c2] bg-[#fff1f1] px-4 py-3 text-sm text-[#a94444]">
                  Desktop runtime is not connected. Run <span className="font-mono">npm run tauri:dev</span> to
                  enable real microphone dictation and model downloads.
                </div>
              )}
              <Dashboard
                theme={theme}
                status={status}
                onPressStart={handlePressStart}
                onPressEnd={handlePressEnd}
                currentModel={settings.activeModel}
                partialTranscript={snapshot.lastPartial}
                finalTranscript={snapshot.lastFinal}
                recentSentences={recentSentences}
                pushToTalkHotkey={settings.pushToTalkHotkey}
                isPro={isPro}
                historyOff={historyPolicy === "off"}
              />
            </>
          )}

          {activeNav === "pro" && (
            <section className="vw-panel vw-panel-soft">
              <p className="vw-kicker">VoiceWave Pro</p>
              <h3 className="vw-section-heading mt-1 text-2xl font-semibold text-[#09090B]">
                Power features for coders and students
              </h3>
              <p className="mt-1 max-w-2xl text-sm text-[#71717A]">
                Advanced formatting, domain packs, code mode, and power history tools.
              </p>

              <div className="vw-ring-shell vw-ring-shell-lg mt-6">
                <div className="vw-ring-inner px-6 py-6">
                  <div className="flex flex-wrap items-start justify-between gap-6">
                    <div className="min-w-0">
                      <div className="flex flex-wrap items-center gap-2">
                        <span className="vw-chip vw-chip-ink">Release Offer</span>
                        <span className="vw-chip vw-chip-accent">{proStatusLabel}</span>
                      </div>
                      <p className="vw-section-heading mt-3 text-2xl font-semibold text-[#09090B]">
                        {releaseOfferHeadline}
                      </p>
                      <p className="mt-2 text-sm text-[#3F3F46]">{releaseOfferLine}</p>
                      <p className="mt-1 text-xs text-[#71717A]">{releaseOfferStateLine}</p>
                    </div>
                    <button type="button" className="vw-btn-primary" onClick={() => setActiveNav("pro-tools")}>
                      Open Pro Tools
                    </button>
                  </div>
                </div>
              </div>

              {isPro && (
                <div className="vw-pro-minimal-grid mt-4">
                  {PRO_HIGHLIGHT_CARDS.map((item) => {
                    const Icon = item.icon;
                    return (
                      <article key={item.id} className="vw-pro-minimal-card">
                        <div className="vw-pro-minimal-icon">
                          <Icon size={15} />
                        </div>
                        <p className="text-sm font-semibold text-[#09090B]">{item.title}</p>
                        <p className="mt-1 text-xs text-[#71717A]">{item.subtitle}</p>
                      </article>
                    );
                  })}
                </div>
              )}

              <div className="mt-8 border-t border-[#F1F1F3] pt-4">
                <button
                  type="button"
                  className="text-xs font-semibold text-[#A1A1AA] transition-colors hover:text-[#52525B]"
                  onClick={() => setOwnerTapCount((count) => Math.min(count + 1, 5))}
                >
                  Owner tools
                </button>
                {showOwnerUnlock ? (
                  <div className="mt-3 flex flex-wrap items-center gap-2">
                    <input
                      type="password"
                      value={ownerPassphrase}
                      onChange={(event) => setOwnerPassphrase(event.target.value)}
                      placeholder="Owner passphrase"
                      className="vw-field"
                    />
                    <button
                      type="button"
                      className="vw-btn-primary vw-btn-sm"
                      onClick={() => void setOwnerOverride(true, ownerPassphrase)}
                    >
                      Enable Owner Pro
                    </button>
                    <button
                      type="button"
                      className="vw-btn-secondary vw-btn-sm"
                      onClick={() => void setOwnerOverride(false, ownerPassphrase)}
                    >
                      Disable
                    </button>
                  </div>
                ) : null}
              </div>
            </section>
          )}

          {activeNav === "models" && (
            <>
              <section className="vw-panel vw-panel-soft">
                <div className="flex flex-wrap items-end justify-between gap-3">
                  <div>
                    <p className="vw-kicker">On-device Models</p>
                    <h3 className="vw-section-heading mt-1 text-2xl font-semibold text-[#09090B]">Model Manager</h3>
                    <p className="mt-1 text-sm text-[#71717A]">
                      {installedModels.length} of {modelCatalog.length} installed · active{" "}
                      <span className="font-semibold text-[#09090B]">{settings.activeModel}</span>
                    </p>
                  </div>
                  <button type="button" className="vw-btn-secondary vw-btn-sm" onClick={() => void refreshPhase3Data()}>
                    Refresh
                  </button>
                </div>

                <div className="vw-row-list mt-5">
                  {modelCatalog.map((model) => {
                    const statusRow = modelStatuses[model.modelId];
                    const isInstalled = installedModelSet.has(model.modelId);
                    const isActiveModel = settings.activeModel === model.modelId;
                    const isBusy = statusRow?.state === "downloading" || statusRow?.state === "paused";
                    const canInstall = !isInstalled && !isBusy;
                    const installLabel =
                      statusRow?.state === "failed" || statusRow?.state === "cancelled" ? "Retry" : "Install";
                    const statusLabel = statusRow?.state ?? (isInstalled ? "installed" : "not installed");
                    const statusDotClass =
                      statusRow?.state === "downloading"
                        ? "bg-[#1B8EFF]"
                        : statusRow?.state === "failed" || statusRow?.state === "cancelled"
                          ? "bg-[#EF4444]"
                          : isInstalled
                            ? "bg-[#10B981]"
                            : "bg-[#D4D4D8]";
                    return (
                      <div key={model.modelId} className="px-5 py-4">
                        <div className="flex flex-wrap items-center justify-between gap-3">
                          <div className="min-w-0">
                            <div className="flex flex-wrap items-center gap-2">
                              <p className="text-sm font-semibold text-[#09090B]">{model.displayName}</p>
                              {isActiveModel && <span className="vw-chip vw-chip-ink">Active</span>}
                              {!isActiveModel && model.modelId === "fw-small.en" && (
                                <span className="vw-chip" title="Default model. Fast, accurate, works on any machine.">
                                  Recommended
                                </span>
                              )}
                            </div>
                            <p className="mt-1 flex items-center gap-2 text-xs text-[#71717A]">
                              <span className={`inline-block h-1.5 w-1.5 rounded-full ${statusDotClass}`} />
                              <span className="capitalize">{statusLabel}</span>
                              <span className="text-[#D4D4D8]">·</span>
                              <span>
                                v{model.version} · {formatBytes(model.sizeBytes)} · {model.license}
                              </span>
                            </p>
                            {statusRow?.message && <p className="mt-1 text-xs text-[#71717A]">{statusRow.message}</p>}
                          </div>
                          <div className="flex flex-wrap items-center gap-2">
                            {canInstall && (
                              <button
                                type="button"
                                className="vw-btn-primary vw-btn-sm"
                                onClick={() => void installModel(model.modelId)}
                              >
                                {installLabel}
                              </button>
                            )}
                            {statusRow?.state === "downloading" && (
                              <button
                                type="button"
                                className="vw-btn-secondary vw-btn-sm"
                                onClick={() => void pauseModelInstall(model.modelId)}
                              >
                                Pause
                              </button>
                            )}
                            {statusRow?.state === "paused" && (
                              <button
                                type="button"
                                className="vw-btn-secondary vw-btn-sm"
                                onClick={() => void resumeModelInstall(model.modelId)}
                              >
                                Resume
                              </button>
                            )}
                            {(statusRow?.state === "downloading" ||
                              statusRow?.state === "paused" ||
                              statusRow?.state === "failed" ||
                              statusRow?.state === "cancelled") && (
                              <button
                                type="button"
                                className="vw-btn-danger vw-btn-sm"
                                onClick={() => void cancelModelInstall(model.modelId)}
                              >
                                Cancel
                              </button>
                            )}
                            {isInstalled && !isActiveModel && (
                              <button
                                type="button"
                                className="vw-btn-secondary vw-btn-sm"
                                onClick={() => void makeModelActive(model.modelId)}
                              >
                                Make Active
                              </button>
                            )}
                          </div>
                        </div>
                        {isBusy &&
                          typeof statusRow?.downloadedBytes === "number" &&
                          typeof statusRow?.totalBytes === "number" &&
                          statusRow.totalBytes > 0 && (
                            <div className="mt-3">
                              <div className="vw-progress">
                                <div
                                  className="vw-progress-fill vw-progress-fill-accent"
                                  style={{
                                    width: `${Math.min(
                                      100,
                                      Math.round((statusRow.downloadedBytes / statusRow.totalBytes) * 100)
                                    )}%`
                                  }}
                                />
                              </div>
                              <p className="mt-1.5 text-[11px] text-[#71717A]">
                                {formatBytes(statusRow.downloadedBytes)} / {formatBytes(statusRow.totalBytes)}
                                {statusRow.state === "downloading" &&
                                  typeof modelSpeeds[model.modelId] === "number" && (
                                    <span className="ml-2">{formatBytes(Math.round(modelSpeeds[model.modelId]))}/s</span>
                                  )}
                              </p>
                            </div>
                          )}
                      </div>
                    );
                  })}
                </div>
              </section>

              <section className="vw-panel vw-panel-soft mt-2 pt-0">
                <div className="vw-surface-base">
                  <button
                    type="button"
                    className="flex w-full items-center justify-between gap-3 px-5 py-4 text-left"
                    onClick={() => setBenchmarkPanelOpen((open) => !open)}
                    aria-expanded={benchmarkPanelOpen}
                  >
                    <div>
                      <p className="text-sm font-semibold text-[#09090B]">Benchmark Recommendation</p>
                      <p className="mt-0.5 text-xs text-[#71717A]">
                        {modelRecommendation
                          ? `Recommended: ${modelRecommendation.modelId}`
                          : "Run a local benchmark to find the best model for this machine."}
                      </p>
                    </div>
                    <ChevronDown
                      size={16}
                      className={`shrink-0 text-[#71717A] transition-transform ${benchmarkPanelOpen ? "rotate-180" : ""}`}
                    />
                  </button>

                  {benchmarkPanelOpen && (
                    <div className="border-t border-[#F1F1F3] px-5 py-4">
                      <button
                        type="button"
                        className="vw-btn-primary vw-btn-sm"
                        onClick={() => void runBenchmarkAndRecommend()}
                      >
                        Run Benchmark
                      </button>

                      {modelRecommendation && (
                        <p className="mt-3 text-sm text-[#3F3F46]">
                          <span className="font-semibold text-[#09090B]">Recommended: {modelRecommendation.modelId}</span>
                          {" — "}
                          {modelRecommendation.reason}
                        </p>
                      )}

                      {benchmarkResults && (
                        <div className="mt-4 overflow-x-auto rounded-xl border border-[#F1F1F3]">
                          <table className="w-full text-left text-sm">
                            <thead className="text-xs uppercase tracking-wide text-[#A1A1AA]">
                              <tr>
                                <th className="px-3 py-2 font-semibold">Model</th>
                                <th className="px-3 py-2 font-semibold">P50</th>
                                <th className="px-3 py-2 font-semibold">P95</th>
                                <th className="px-3 py-2 font-semibold">Avg RTF</th>
                              </tr>
                            </thead>
                            <tbody>
                              {benchmarkResults.rows.map((row) => (
                                <tr key={row.modelId} className="border-t border-[#F1F1F3] text-[#09090B]">
                                  <td className="px-3 py-2">{row.modelId}</td>
                                  <td className="px-3 py-2 tabular-nums">{row.p50LatencyMs} ms</td>
                                  <td className="px-3 py-2 tabular-nums">{row.p95LatencyMs} ms</td>
                                  <td className="px-3 py-2 tabular-nums">{row.averageRtf.toFixed(2)}</td>
                                </tr>
                              ))}
                            </tbody>
                          </table>
                        </div>
                      )}
                    </div>
                  )}
                </div>
              </section>
            </>
          )}

          {activeNav === "sessions" && (
            <section className="vw-panel vw-panel-soft">
              <p className="vw-kicker">Local Only</p>
              <h3 className="vw-section-heading mt-1 text-2xl font-semibold text-[#09090B]">History</h3>
              <p className="mt-1 text-sm text-[#71717A]">
                Every dictation stays on this machine. Retention is under your control.
              </p>

              {historyPolicy === "off" && (
                <div className="mt-4 rounded-2xl border border-[#FDE68A] bg-[#FFFBEB] px-4 py-3">
                  <div className="flex flex-wrap items-center justify-between gap-3">
                    <div>
                      <p className="text-sm font-semibold text-[#92400E]">History is off</p>
                      <p className="mt-1 text-sm text-[#92400E]">
                        Dictations are not being saved. Turn retention on to see your transcripts here and on the dashboard.
                      </p>
                    </div>
                    <button
                      type="button"
                      className="vw-btn-primary vw-btn-sm"
                      onClick={() => void updateRetentionPolicy("days30")}
                    >
                      Keep 30 days
                    </button>
                  </div>
                </div>
              )}

              <div className="mt-5 grid grid-cols-1 gap-3 md:grid-cols-3">
                <div className="vw-stat-card">
                  <p className="vw-kicker">Retention</p>
                  <p className="mt-1 text-lg font-semibold text-[#09090B]">{policyLabel(historyPolicy)}</p>
                </div>
                <div className="vw-stat-card">
                  <p className="vw-kicker">Records</p>
                  <p className="mt-1 text-lg font-semibold tabular-nums text-[#09090B]">{sessionHistory.length}</p>
                </div>
                <div className="vw-stat-card">
                  <p className="vw-kicker">Success Ratio</p>
                  <p className="mt-1 text-lg font-semibold tabular-nums text-[#09090B]">
                    {sessionHistory.length === 0
                      ? "—"
                      : `${Math.round(
                          (sessionHistory.filter((record) => record.success).length / sessionHistory.length) * 100
                        )}%`}
                  </p>
                </div>
              </div>

              <div className="mt-4 flex flex-wrap items-center justify-between gap-3">
                <div className="vw-seg" role="group" aria-label="History retention policy">
                  {retentionOptions.map((option) => (
                    <button
                      key={option}
                      type="button"
                      className={`vw-seg-btn ${historyPolicy === option ? "vw-seg-btn-active" : ""}`}
                      onClick={() => void updateRetentionPolicy(option)}
                    >
                      {policyLabel(option)}
                    </button>
                  ))}
                </div>
                <div className="flex flex-wrap gap-2">
                  <button type="button" className="vw-btn-secondary vw-btn-sm" onClick={() => void pruneHistory()}>
                    Prune Now
                  </button>
                  <button type="button" className="vw-btn-danger vw-btn-sm" onClick={() => void clearSessionHistory()}>
                    Clear All
                  </button>
                </div>
              </div>

              <div className="vw-surface-base mt-5 px-5 py-4">
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <p className="text-sm font-semibold text-[#09090B]">Search &amp; Export</p>
                  <span className={`vw-chip ${isPro ? "vw-chip-ink" : ""}`}>{isPro ? "Pro Unlocked" : "Pro"}</span>
                </div>
                <div className="mt-3 grid grid-cols-1 gap-2 md:grid-cols-[1fr_200px_auto]">
                  <input
                    value={historyQuery}
                    onChange={(event) => setHistoryQuery(event.target.value)}
                    placeholder="Search transcripts"
                    className="vw-field"
                  />
                  <input
                    value={historyTag}
                    onChange={(event) => setHistoryTag(event.target.value)}
                    placeholder="Tag filter"
                    className="vw-field"
                  />
                  <button
                    type="button"
                    className="vw-btn-primary"
                    onClick={() => {
                      if (!isPro) {
                        setActiveNav("pro");
                        return;
                      }
                      const tags = historyTag.trim() ? [historyTag.trim()] : null;
                      void searchHistory(historyQuery, tags, null);
                    }}
                  >
                    {isPro ? "Search" : "Open Pro Offer"}
                  </button>
                </div>
                <div className="mt-3 flex flex-wrap items-center gap-2">
                  <span className="text-xs text-[#A1A1AA]">Export as</span>
                  {(["plain", "markdownNotes", "studySummary"] as const).map((preset) => (
                    <button
                      key={preset}
                      type="button"
                      className="vw-btn-secondary vw-btn-sm"
                      onClick={() => {
                        if (!isPro) {
                          setActiveNav("pro");
                          return;
                        }
                        void exportHistoryPreset(preset);
                      }}
                    >
                      {preset === "plain" ? "Plain text" : preset === "markdownNotes" ? "Markdown notes" : "Study summary"}
                    </button>
                  ))}
                </div>
                {!isPro && (
                  <p className="mt-2 text-xs text-[#71717A]">
                    Search, tagging, starring, and exports are Pro features. Free keeps the full timeline and retention controls.
                  </p>
                )}
                {lastHistoryExport && (
                  <div className="mt-3 rounded-xl border border-[#F1F1F3] bg-[#FAFAFA] px-3 py-2">
                    <p className="text-xs font-semibold text-[#09090B]">
                      Export ready: {lastHistoryExport.preset} ({lastHistoryExport.recordCount} records)
                    </p>
                    <pre className="mt-2 max-h-28 overflow-auto whitespace-pre-wrap text-[11px] text-[#52525B]">
                      {lastHistoryExport.content}
                    </pre>
                  </div>
                )}
              </div>

              {sessionHistory.length === 0 ? (
                <div className="mt-5 rounded-2xl border border-dashed border-[#E4E4E7] px-6 py-10 text-center">
                  <p className="text-sm font-medium text-[#09090B]">No sessions yet</p>
                  <p className="mt-1 text-sm text-[#71717A]">Dictations will appear here as you use VoiceWave.</p>
                </div>
              ) : (
                <div className="vw-row-list vw-list-stagger mt-5">
                  {sessionHistory.map((record) => (
                    <div key={record.recordId} className="vw-interactive-row px-5 py-3.5">
                      <div className="flex items-start justify-between gap-4">
                        <div className="min-w-0">
                          <p className="truncate text-sm text-[#09090B]">{record.preview}</p>
                          <p className="mt-1 flex flex-wrap items-center gap-x-2 gap-y-1 text-[11px] text-[#A1A1AA]">
                            <span>{formatDate(record.timestampUtcMs)}</span>
                            <span className="text-[#E4E4E7]">·</span>
                            <span className="capitalize">{record.source}</span>
                            {record.method && (
                              <>
                                <span className="text-[#E4E4E7]">·</span>
                                <span>{record.method}</span>
                              </>
                            )}
                            {!record.success && (
                              <>
                                <span className="text-[#E4E4E7]">·</span>
                                <span className="font-semibold text-[#B3261E]">failed</span>
                              </>
                            )}
                            {record.tags.map((tag) => (
                              <span key={`${record.recordId}-${tag}`} className="vw-chip">
                                #{tag}
                              </span>
                            ))}
                          </p>
                        </div>
                        <div className="flex shrink-0 items-center gap-1">
                          <button
                            type="button"
                            className="vw-icon-btn"
                            aria-label="Copy transcript"
                            title={copiedHistoryId === record.recordId ? "Copied" : "Copy transcript"}
                            onClick={() => {
                              const fullText = record.text?.length ? record.text : record.preview;
                              void (async () => {
                                try {
                                  await copyTextToClipboard(fullText);
                                } catch {
                                  try {
                                    await navigator.clipboard.writeText(fullText);
                                  } catch {
                                    return;
                                  }
                                }
                                setCopiedHistoryId(record.recordId);
                                window.setTimeout(
                                  () =>
                                    setCopiedHistoryId((prev) =>
                                      prev === record.recordId ? null : prev
                                    ),
                                  1500
                                );
                              })();
                            }}
                          >
                            {copiedHistoryId === record.recordId ? (
                              <Check size={15} className="text-[#16A34A]" />
                            ) : (
                              <Copy size={15} />
                            )}
                          </button>
                          <button
                            type="button"
                            className="vw-icon-btn"
                            aria-label={record.starred ? "Unstar session" : "Star session"}
                            title={record.starred ? "Unstar" : "Star"}
                            onClick={() => {
                              if (!isPro) {
                                setActiveNav("pro");
                                return;
                              }
                              void setSessionStarred(record.recordId, !record.starred);
                            }}
                          >
                            <Star
                              size={15}
                              className={record.starred ? "fill-[#09090B] text-[#09090B]" : ""}
                            />
                          </button>
                          <button
                            type="button"
                            className="vw-btn-secondary vw-btn-sm"
                            title="Apply the tag from the tag filter box"
                            onClick={() => {
                              if (!isPro) {
                                setActiveNav("pro");
                                return;
                              }
                              if (!historyTag.trim()) {
                                return;
                              }
                              void addSessionTag(record.recordId, historyTag.trim());
                            }}
                          >
                            Tag
                          </button>
                        </div>
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </section>
          )}

          {activeNav === "dictionary" && (
            <section className="vw-panel vw-panel-soft">
              <p className="vw-kicker">{cloudUserId ? "Synced Across Devices" : "Device Local"}</p>
              <h3 className="vw-section-heading mt-1 text-2xl font-semibold text-[#09090B]">Personal Dictionary</h3>
              <p className="mt-1 text-sm text-[#71717A]">
                {activeDictionaryTerms.length} approved {activeDictionaryTerms.length === 1 ? "term" : "terms"}
                {dictionaryQueue.length > 0 ? ` · ${dictionaryQueue.length} pending review` : ""} — new suggestions
                surface in the floating pill.
              </p>

              <div className="vw-surface-base mt-5 px-5 py-4">
                <div className="flex flex-wrap gap-2">
                  <input
                    value={dictionaryDraftTerm}
                    onChange={(event) => setDictionaryDraftTerm(event.target.value)}
                    onKeyDown={(event) => {
                      if (event.key === "Enter") {
                        event.preventDefault();
                        submitDictionaryDraft();
                      }
                    }}
                    placeholder="Add a name, product, or term VoiceWave should always spell right"
                    className="vw-field min-w-[220px] flex-1"
                  />
                  <button type="button" className="vw-btn-primary" onClick={submitDictionaryDraft}>
                    Add
                  </button>
                </div>
                <p className="mt-2 text-xs text-[#71717A]">
                  {cloudUserId
                    ? "New terms are saved to your account and follow you to every install."
                    : "Sign in to sync dictionary terms across devices."}
                </p>
              </div>

              <div className="vw-surface-base mt-4 px-5 py-4">
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <div>
                    <p className="text-sm font-semibold text-[#09090B]">Domain packs</p>
                    <p className="mt-0.5 text-xs text-[#71717A]">
                      Curated vocabulary for your field, applied on top of your own terms.
                    </p>
                  </div>
                  <span className={`vw-chip ${isPro ? "vw-chip-ink" : ""}`}>{isPro ? "Pro Unlocked" : "Pro"}</span>
                </div>
                <div className="mt-3 flex flex-wrap gap-2">
                  {domainPackOptions.map((pack) => {
                    const active = settings.activeDomainPacks.includes(pack);
                    return (
                      <button
                        key={pack}
                        type="button"
                        className={`vw-seg-btn ${active ? "vw-seg-btn-active" : ""} border border-[#E4E4E7] capitalize`}
                        aria-pressed={active}
                        onClick={() => {
                          if (!isPro) {
                            setActiveNav("pro");
                            return;
                          }
                          const next = active
                            ? settings.activeDomainPacks.filter((value) => value !== pack)
                            : [...settings.activeDomainPacks, pack];
                          void setDomainPacks(next);
                        }}
                      >
                        {pack}
                      </button>
                    );
                  })}
                </div>
              </div>

              <div className="mt-4">
                <div className="flex items-center justify-between gap-2 px-1 pb-2">
                  <h4 className="text-sm font-semibold text-[#09090B]">Approved terms</h4>
                  <span className="text-xs tabular-nums text-[#A1A1AA]">{sortedDictionaryTerms.length} total</span>
                </div>
                {sortedDictionaryTerms.length === 0 ? (
                  <div className="rounded-2xl border border-dashed border-[#E4E4E7] px-6 py-8 text-center">
                    <p className="text-sm font-medium text-[#09090B]">No approved terms yet</p>
                    <p className="mt-1 text-sm text-[#71717A]">
                      Add one above, or approve suggestions from the pill after dictating.
                    </p>
                  </div>
                ) : (
                  <div className="vw-row-list vw-list-stagger max-h-[380px] overflow-y-auto">
                    {sortedDictionaryTerms.map((term) => (
                      <div key={term.termId} className="vw-interactive-row flex items-center justify-between gap-3 px-4 py-2.5">
                        <div className="min-w-0">
                          <p className="truncate text-sm font-medium text-[#09090B]">{term.term}</p>
                          <p className="text-[11px] text-[#A1A1AA]">{term.source}</p>
                        </div>
                        <button
                          type="button"
                          className="vw-icon-btn"
                          aria-label={`Remove ${term.term}`}
                          title="Remove term"
                          onClick={() => handleDeleteDictionaryTerm(term.termId)}
                        >
                          <X size={14} />
                        </button>
                      </div>
                    ))}
                  </div>
                )}
              </div>

              <div className="vw-surface-base mt-4">
                <button
                  type="button"
                  className="flex w-full items-center justify-between gap-2 px-5 py-4 text-left"
                  onClick={() => setDictionaryPendingOpen((open) => !open)}
                  aria-expanded={dictionaryPendingOpen}
                >
                  <p className="text-sm font-semibold text-[#09090B]">Pending review queue</p>
                  <span className="flex items-center gap-2 text-xs text-[#71717A]">
                    {dictionaryQueue.length} {dictionaryQueue.length === 1 ? "item" : "items"}
                    <ChevronDown
                      size={14}
                      className={`transition-transform ${dictionaryPendingOpen ? "rotate-180" : ""}`}
                    />
                  </span>
                </button>
                {dictionaryPendingOpen && (
                  <div className="border-t border-[#F1F1F3]">
                    {sortedDictionaryQueue.length === 0 && (
                      <p className="px-5 py-4 text-sm text-[#71717A]">Queue is empty.</p>
                    )}
                    {sortedDictionaryQueue.map((item) => (
                      <div
                        key={item.entryId}
                        className="flex items-center justify-between gap-3 border-b border-[#F1F1F3] px-5 py-3 last:border-b-0"
                      >
                        <div className="min-w-0">
                          <p className="text-sm font-medium text-[#09090B]">{item.term}</p>
                          <p className="mt-0.5 truncate text-xs text-[#A1A1AA]">{item.sourcePreview}</p>
                        </div>
                        <div className="flex shrink-0 gap-2">
                          <button
                            type="button"
                            className="vw-btn-primary vw-btn-sm"
                            onClick={() => handleApproveDictionaryQueueEntry(item.entryId)}
                          >
                            Approve
                          </button>
                          <button
                            type="button"
                            className="vw-btn-danger vw-btn-sm"
                            onClick={() => void rejectDictionaryQueueEntry(item.entryId)}
                          >
                            Dismiss
                          </button>
                        </div>
                      </div>
                    ))}
                  </div>
                )}
              </div>

              <div className="mt-4 flex flex-wrap items-center justify-between gap-3 px-1">
                <p className="text-xs text-[#A1A1AA]">
                  Back up approved terms as JSON, or restore them on a new install. Duplicates are skipped.
                </p>
                <div className="flex gap-2">
                  <button type="button" className="vw-btn-secondary vw-btn-sm" onClick={handleExportDictionary}>
                    Export
                  </button>
                  <button
                    type="button"
                    className="vw-btn-secondary vw-btn-sm"
                    onClick={() => dictionaryImportInputRef.current?.click()}
                  >
                    Import
                  </button>
                  <input
                    ref={dictionaryImportInputRef}
                    type="file"
                    accept="application/json"
                    className="hidden"
                    onChange={handleImportDictionaryFile}
                  />
                </div>
              </div>
              {dictionaryPortNotice && <p className="mt-2 px-1 text-xs text-[#71717A]">{dictionaryPortNotice}</p>}
            </section>
          )}

          {activeNav === "pro-tools" && (
            <>
              <section className="vw-panel vw-panel-soft">
                <div className="flex flex-wrap items-end justify-between gap-2">
                  <div>
                    <p className="vw-kicker">Workflows</p>
                    <h3 className="vw-section-heading mt-1 text-2xl font-semibold text-[#09090B]">Pro Tools Modes</h3>
                    <p className="mt-1 text-sm text-[#71717A]">
                      Pick one mode and VoiceWave reconfigures output behavior for that workflow.
                    </p>
                  </div>
                  <span className="vw-chip vw-chip-ink">Pro Active</span>
                </div>

                <div className="mt-5 grid grid-cols-1 gap-3 md:grid-cols-2">
                  {PRO_TOOLS_MODE_CARDS.map((mode) => {
                    const isActiveMode = displayedProToolsMode === mode.id;
                    const isApplying = modeApplyPending === mode.id;
                    return (
                      <button
                        key={mode.id}
                        type="button"
                        className={`vw-mode-card rounded-2xl border px-4 py-4 text-left ${
                          isActiveMode ? "vw-pro-mode-card-active" : ""
                        }`}
                        onClick={() => void applyProToolsMode(mode.id)}
                        aria-disabled={modeApplyPending ? "true" : "false"}
                        aria-busy={isApplying ? "true" : "false"}
                      >
                        <div className="flex items-center justify-between gap-2">
                          <p className="text-base font-semibold text-[#09090B]">{mode.title}</p>
                          <span className={`vw-chip vw-mode-status-chip ${isActiveMode ? "vw-mode-status-chip-active" : ""}`}>
                            {isActiveMode ? "Active" : "Apply"}
                          </span>
                        </div>
                        <p className="mt-2 text-sm text-[#3F3F46]">{mode.description}</p>
                        <p className="mt-2 text-xs text-[#71717A]">{mode.highlight}</p>
                      </button>
                    );
                  })}
                </div>

                {displayedProToolsMode === "coding" && (
                  <div className="vw-surface-elevated mt-4 rounded-2xl border border-[#E4E4E7] bg-white px-4 py-3">
                    <p className="text-sm font-semibold text-[#09090B]">How To Speak In Coding Mode</p>
                    <div className="mt-2 grid grid-cols-1 gap-2 text-xs text-[#52525B] md:grid-cols-2">
                      <p><span className="font-semibold">Symbols:</span> open paren, open parenthesis, close paren, underscore, arrow, equals.</p>
                      <p><span className="font-semibold">Casing:</span> say plain words, then choose camelCase or snake_case in mode settings.</p>
                      <p><span className="font-semibold">Example speech:</span> open paren user id close paren arrow result</p>
                      <p><span className="font-semibold">Expected output:</span> (user id)-&gt;result</p>
                    </div>
                  </div>
                )}

                {displayedProToolsMode === "writing" && (
                  <div className="vw-surface-elevated mt-4 rounded-2xl border border-[#E4E4E7] bg-white px-4 py-3">
                    <p className="text-sm font-semibold text-[#09090B]">Writing Mode Focus</p>
                    <p className="mt-2 text-xs text-[#52525B]">
                      List intent is detected more strongly. Example: "there are two process one hi two real" becomes:
                      <br />
                      1. Hi
                      <br />
                      2. Real
                    </p>
                  </div>
                )}

                {displayedProToolsMode === "study" && (
                  <div className="vw-surface-elevated mt-4 rounded-2xl border border-[#E4E4E7] bg-white px-4 py-3">
                    <p className="text-sm font-semibold text-[#09090B]">Study Mode Focus</p>
                    <p className="mt-2 text-xs text-[#52525B]">
                      Designed for voice notes you can revise later. Speak with markers like:
                      <span className="font-semibold"> topic</span>, <span className="font-semibold">definition</span>, <span className="font-semibold">example</span>, and <span className="font-semibold">summary</span>.
                    </p>
                  </div>
                )}
              </section>
            </>
          )}
        </div>

        {displayError && (
          <div className="mt-6 rounded-2xl border border-[#f3c2c2] bg-[#fff1f1] px-4 py-3 text-sm text-[#a94444]">
            <p>{displayError}</p>
            {proRequiredFeature && (
              <button
                type="button"
                className="vw-btn-primary vw-action-button mt-3"
                onClick={() => setActiveNav("pro")}
              >
                Open Pro Offer
              </button>
            )}
          </div>
        )}
        {cloudSyncError && (
          <div className="mt-3 rounded-2xl border border-[#FDE68A] bg-[#FFFBEB] px-4 py-3 text-sm text-[#92400E]">
            <p>Cloud sync warning: {cloudSyncError}</p>
          </div>
        )}
      </Layout>

      {activeOverlay === "settings" && (
        <OverlayModal
          title="Settings"
          subtitle="Tune how VoiceWave listens, inserts, and polishes."
          onClose={closeOverlay}
          maxWidthClassName="max-w-4xl"
        >
          <div className="vw-settings-shell">
            <nav className="vw-settings-rail" aria-label="Settings sections">
              {SETTINGS_SECTIONS.map((section) => {
                const SectionIcon = section.icon;
                return (
                  <button
                    key={section.id}
                    type="button"
                    className="vw-settings-rail-btn"
                    aria-selected={settingsSection === section.id}
                    onClick={() => setSettingsSection(section.id)}
                  >
                    <SectionIcon size={15} className={settingsSection === section.id ? "text-[#1B8EFF]" : "text-[#A1A1AA]"} />
                    {section.label}
                  </button>
                );
              })}
            </nav>

            <div className="vw-settings-panel">
              {settingsSection === "audio" && (
                <div>
                  <div className="vw-set-row">
                    <div>
                      <p className="vw-set-title">Microphone input</p>
                      <p className="vw-set-desc">The device VoiceWave listens to during dictation.</p>
                    </div>
                    <div className="flex items-center gap-2">
                      <select
                        className="vw-field max-w-[240px]"
                        aria-label="Microphone input device"
                        value={settings.inputDevice ?? ""}
                        onChange={(event) => void setInputDevice(event.target.value ? event.target.value : null)}
                      >
                        <option value="">Default system input</option>
                        {inputDevices.map((device) => (
                          <option key={device} value={device}>
                            {device}
                          </option>
                        ))}
                      </select>
                      <button type="button" className="vw-btn-secondary vw-btn-sm" onClick={() => void refreshInputDevices()}>
                        Refresh
                      </button>
                    </div>
                  </div>
                  {inputDevices.length === 0 && (
                    <p className="pb-3 text-xs text-[#B3261E]">No input devices detected.</p>
                  )}

                  <div className="vw-set-row">
                    <div>
                      <p className="vw-set-title">Volume guard</p>
                      <p className="vw-set-desc">
                        {settings.micVolumeGuard === "off"
                          ? "Call apps can silently lower your Windows mic volume. VoiceWave will not check it."
                          : settings.micVolumeGuard === "autoRestore"
                          ? "Restores mic volume to 100% automatically and tells you it happened."
                          : "Shows a pill notice when another app has lowered your mic volume."}
                      </p>
                    </div>
                    <div className="vw-seg" role="group" aria-label="Microphone volume guard">
                      {(["off", "warn", "autoRestore"] as MicVolumeGuardMode[]).map((mode) => (
                        <button
                          key={mode}
                          type="button"
                          className={`vw-seg-btn ${settings.micVolumeGuard === mode ? "vw-seg-btn-active" : ""}`}
                          onClick={() => void setMicVolumeGuard(mode)}
                        >
                          {mode === "off" ? "Off" : mode === "warn" ? "Warn" : "Auto-restore"}
                        </button>
                      ))}
                    </div>
                  </div>

                  {micQualityWarning && (
                    <div className="mt-4 rounded-2xl border border-[#FDE68A] bg-[#FFFBEB] px-4 py-3">
                      <p className="text-sm font-semibold text-[#92400E]">Microphone quality warning</p>
                      <p className="mt-1 text-sm text-[#92400E]">{micQualityWarning.message}</p>
                      <p className="mt-2 text-xs text-[#A16207]">Current input: {micQualityWarning.currentDevice}</p>
                      {micQualityWarning.recommendedDevice && (
                        <p className="mt-1 text-xs text-[#A16207]">Suggested input: {micQualityWarning.recommendedDevice}</p>
                      )}
                      <div className="mt-3 flex flex-wrap gap-2">
                        {micQualityWarning.recommendedDevice && (
                          <button type="button" className="vw-btn-primary vw-btn-sm" onClick={() => void switchToRecommendedInput()}>
                            Switch to Suggested Input
                          </button>
                        )}
                        <button type="button" className="vw-btn-secondary vw-btn-sm" onClick={() => void refreshInputDevices()}>
                          Refresh Devices
                        </button>
                      </div>
                    </div>
                  )}
                </div>
              )}

              {settingsSection === "dictation" && (
                <div>
                  <div className="vw-set-row">
                    <div>
                      <p className="vw-set-title">Permissions</p>
                      <p className="vw-set-desc">
                        Microphone {permissions.microphone} · Insertion {permissions.insertionCapability}
                        {permissions.message ? ` — ${permissions.message}` : ""}
                      </p>
                    </div>
                    <button type="button" className="vw-btn-secondary vw-btn-sm" onClick={() => void requestMicAccess()}>
                      Check Microphone Permission
                    </button>
                  </div>

                  <div className="vw-set-row">
                    <div>
                      <p className="vw-set-title">Spoken edit commands</p>
                      <p className="vw-set-desc">
                        Say "new line", "new paragraph", or "bullet point" to add structure while dictating.
                      </p>
                    </div>
                    <span className="vw-switch">
                      <input
                        type="checkbox"
                        aria-label="Enable spoken edit commands"
                        checked={settings.spokenEditCommands}
                        onChange={(event) => void setSpokenEditCommands(event.target.checked)}
                      />
                      <span className="vw-switch-track" aria-hidden="true" />
                    </span>
                  </div>

                  <div className="vw-set-row">
                    <div>
                      <p className="vw-set-title">Prefer clipboard insertion</p>
                      <p className="vw-set-desc">
                        Paste through the clipboard first instead of simulated typing. More reliable in some apps.
                      </p>
                    </div>
                    <span className="vw-switch">
                      <input
                        type="checkbox"
                        aria-label="Prefer clipboard fallback for insertion"
                        checked={settings.preferClipboardFallback}
                        onChange={(event) => void setPreferClipboardFallback(event.target.checked)}
                      />
                      <span className="vw-switch-track" aria-hidden="true" />
                    </span>
                  </div>
                </div>
              )}

              {settingsSection === "polish" && (
                <div>
                  <div className="vw-set-row">
                    <div>
                      <p className="vw-set-title">On-device AI polish</p>
                      <p className="vw-set-desc">
                        After dictation, a local model offers a cleaned-up version in the pill. Nothing is sent
                        to the cloud. Needs a one-time ~1 GB model download.
                      </p>
                    </div>
                    <span className="vw-switch">
                      <input
                        type="checkbox"
                        aria-label="Enable on-device AI polish"
                        checked={settings.llmPolishEnabled ?? false}
                        onChange={(event) => void setLlmPolishEnabled(event.target.checked)}
                      />
                      <span className="vw-switch-track" aria-hidden="true" />
                    </span>
                  </div>

                  {polishModelProgress && !polishModelProgress.done && !polishModelProgress.error && (
                    <div className="mt-2">
                      <div className="vw-progress">
                        <div
                          className="vw-progress-fill"
                          style={{
                            width: `${
                              polishModelProgress.total > 0
                                ? Math.min(
                                    100,
                                    Math.round((polishModelProgress.downloaded / polishModelProgress.total) * 100)
                                  )
                                : 3
                            }%`
                          }}
                        />
                      </div>
                      <p className="mt-1.5 text-xs text-[#71717A]">
                        {polishModelProgress.total > 0
                          ? `Downloading AI polish model… ${Math.round(
                              (polishModelProgress.downloaded / polishModelProgress.total) * 100
                            )}%`
                          : "Preparing AI polish model download…"}
                      </p>
                    </div>
                  )}
                  {polishModelProgress?.done && (
                    <p className="mt-2 text-xs font-medium text-[#15803D]">AI polish model ready.</p>
                  )}
                  {polishModelProgress?.error && (
                    <p className="mt-2 text-xs text-[#B3261E]">Model download failed: {polishModelProgress.error}</p>
                  )}
                </div>
              )}

              {settingsSection === "diagnostics" && (
                <div>
                  <div className="vw-set-row">
                    <div>
                      <p className="vw-set-title">Diagnostics</p>
                      <p className="vw-set-desc">
                        Keep an encrypted local log of dictation runs to help debug transcription issues.
                      </p>
                    </div>
                    <span className="vw-switch">
                      <input
                        type="checkbox"
                        aria-label="Enable diagnostics"
                        checked={settings.diagnosticsOptIn}
                        onChange={(event) => void setDiagnosticsOptIn(event.target.checked)}
                      />
                      <span className="vw-switch-track" aria-hidden="true" />
                    </span>
                  </div>

                  <div className="vw-set-row">
                    <div>
                      <p className="vw-set-title">Export bundle</p>
                      <p className="vw-set-desc">
                        {diagnosticsStatus.recordCount} records · {diagnosticsStatus.watchdogRecoveryCount} watchdog
                        recoveries · Last export{" "}
                        {diagnosticsStatus.lastExportedAtUtcMs
                          ? formatDate(diagnosticsStatus.lastExportedAtUtcMs)
                          : "never"}
                      </p>
                    </div>
                    <button type="button" className="vw-btn-secondary vw-btn-sm" onClick={() => void exportDiagnosticsBundle()}>
                      Export Diagnostics Bundle
                    </button>
                  </div>

                  {lastDiagnosticsExport && (
                    <div className="mt-2 rounded-xl border border-[#E4E4E7] bg-[#FAFAFA] px-3 py-2 text-xs text-[#52525B]">
                      <p>
                        Export complete:{" "}
                        <span className="font-semibold">{formatDate(lastDiagnosticsExport.exportedAtUtcMs)}</span>
                      </p>
                      <p className="mt-1 break-all font-mono">{lastDiagnosticsExport.filePath}</p>
                    </div>
                  )}
                </div>
              )}

              {settingsSection === "advanced" && (
                <div>
                  <div className="vw-set-row">
                    <div>
                      <p className="vw-set-title">VAD Threshold</p>
                      <p className="vw-set-desc">How loud audio must be before it counts as speech.</p>
                    </div>
                    <div className="flex w-56 flex-col items-end gap-1">
                      <input
                        className="w-full accent-[#18181B]"
                        type="range"
                        aria-label="VAD threshold"
                        min={0.005}
                        max={0.04}
                        step={0.001}
                        value={settings.vadThreshold}
                        onChange={(event) => void setVadThreshold(Number(event.target.value))}
                      />
                      <div className="flex items-center gap-2">
                        <span className="text-sm font-semibold text-[#09090B]">{settings.vadThreshold.toFixed(3)}</span>
                        <button type="button" className="vw-btn-secondary vw-btn-sm" onClick={() => void resetVadThreshold()}>
                          Reset to {recommendedVadThreshold.toFixed(3)}
                        </button>
                      </div>
                    </div>
                  </div>

                  <div className="vw-set-row">
                    <div>
                      <p className="vw-set-title">Max Utterance (ms)</p>
                      <p className="vw-set-desc">Longest single capture before VoiceWave finalizes on its own.</p>
                    </div>
                    <div className="flex w-56 flex-col items-end gap-1">
                      <input
                        className="w-full accent-[#18181B]"
                        type="range"
                        aria-label="Max utterance in milliseconds"
                        min={5000}
                        max={180000}
                        step={250}
                        value={settings.maxUtteranceMs}
                        onChange={(event) => void setMaxUtteranceMs(Number(event.target.value))}
                      />
                      <span className="text-sm font-semibold text-[#09090B]">{settings.maxUtteranceMs}</span>
                    </div>
                  </div>

                  <div className="vw-set-row">
                    <div>
                      <p className="vw-set-title">Release Tail (ms)</p>
                      <p className="vw-set-desc">Audio kept after you release push-to-talk so soft word endings land.</p>
                    </div>
                    <div className="flex w-56 flex-col items-end gap-1">
                      <input
                        className="w-full accent-[#18181B]"
                        type="range"
                        aria-label="Release tail in milliseconds"
                        min={120}
                        max={1500}
                        step={10}
                        value={settings.releaseTailMs}
                        onChange={(event) => void setReleaseTailMs(Number(event.target.value))}
                      />
                      <span className="text-sm font-semibold text-[#09090B]">{settings.releaseTailMs}</span>
                    </div>
                  </div>

                  <div className="vw-set-row">
                    <div>
                      <p className="vw-set-title">Audio chunk quality</p>
                      <p className="vw-set-desc">Run a quick capture check with real microphone audio.</p>
                    </div>
                    <button
                      type="button"
                      className="vw-btn-secondary vw-btn-sm"
                      onClick={() => void runAudioQualityDiagnostic(10_000)}
                    >
                      Run 10s Check
                    </button>
                  </div>

                  {audioQualityReport ? (
                    <div className="mt-2 space-y-1 rounded-xl border border-[#E4E4E7] bg-[#FAFAFA] px-3 py-2 text-xs text-[#52525B]">
                      <p>
                        Quality: <span className="font-semibold">{audioQualityReport.quality}</span> · Segments:{" "}
                        {audioQualityReport.segmentCount} · Duration: {(audioQualityReport.durationMs / 1000).toFixed(2)}s
                      </p>
                      <p>
                        RMS: {audioQualityReport.rms.toFixed(3)} · Peak: {audioQualityReport.peak.toFixed(3)} · Clipping:{" "}
                        {(audioQualityReport.clippingRatio * 100).toFixed(1)}%
                      </p>
                      <p>
                        Low-energy frames: {(audioQualityReport.lowEnergyFrameRatio * 100).toFixed(1)}% · SNR proxy:{" "}
                        {audioQualityReport.estimatedSnrDb.toFixed(1)} dB
                      </p>
                    </div>
                  ) : (
                    <p className="mt-2 text-xs text-[#A1A1AA]">No capture diagnostics yet.</p>
                  )}
                  {lastLatency && (
                    <p className="mt-2 text-xs text-[#71717A]">
                      Latest latency: release-to-transcribing {lastLatency.releaseToTranscribingMs} ms, total{" "}
                      {lastLatency.totalMs} ms.
                    </p>
                  )}
                </div>
              )}

              {settingsSection === "updates" && <UpdateSection />}
            </div>
          </div>
        </OverlayModal>
      )}

      {activeOverlay === "profile" && (
        <OverlayModal
          title="Profile"
          subtitle="Your workspace identity and release-offer shortcuts."
          onClose={closeOverlay}
        >
          <div className="space-y-4">
            <section className="rounded-2xl border border-[#E4E4E7] bg-white px-4 py-4">
              <div className="flex flex-wrap items-start justify-between gap-3">
                <div className="flex items-center gap-3">
                  <div className="vw-profile-avatar">
                    {(demoProfile?.name ?? "Guest").slice(0, 1).toUpperCase()}
                  </div>
                  <div>
                    <p className="text-sm font-semibold text-[#09090B]">
                      {demoProfile?.name ?? "Guest Workspace"}
                    </p>
                    <p className="mt-1 text-xs text-[#71717A]">
                      {demoProfile?.email ?? "No sign-in required yet"}
                    </p>
                  </div>
                </div>
                <span className={`vw-chip ${isPro ? "vw-chip-ink" : ""}`}>
                  {isPro ? "Pro Active" : "Free Plan"}
                </span>
              </div>
              <p className="mt-3 text-xs text-[#71717A]">
                {isDemoAuthenticated
                  ? cloudUserId
                    ? "Cloud account is active. Recent sentences and dictionary terms sync automatically."
                    : "Local account mode is active on this device."
                  : "Guest mode is enabled. You can keep using all core flows without signing in."}
              </p>
            </section>

            <section className="vw-profile-quick-grid">
              <button type="button" className="vw-profile-quick-action" onClick={() => openOverlay("settings")}>
                Open Settings
              </button>
              <button
                type="button"
                className="vw-profile-quick-action"
                onClick={() => {
                  closeOverlay();
                  setActiveNav("pro");
                }}
              >
                View Pro Offer
              </button>
              <button
                type="button"
                className="vw-profile-quick-action"
                onClick={() => openAuthOverlay(isDemoAuthenticated ? "signin" : "signup")}
              >
                {isDemoAuthenticated ? "Account Access" : "Sign In / Sign Up"}
              </button>
            </section>

            {isDemoAuthenticated && demoProfile && (
              <section className="rounded-2xl border border-[#E4E4E7] bg-[#FAFAFA] px-4 py-4">
                <p className="text-sm font-semibold text-[#09090B]">Workspace Role</p>
                <p className="mt-1 text-sm text-[#3F3F46]">{demoProfile.workspaceRole}</p>
                <button
                  type="button"
                  className="vw-btn-secondary mt-3"
                  onClick={() => void handleSignOut()}
                  disabled={authPending}
                >
                  {authPending ? "Signing Out..." : "Sign Out"}
                </button>
              </section>
            )}
          </div>
        </OverlayModal>
      )}

      {activeOverlay === "auth" && (
        <OverlayModal
          title={isDemoAuthenticated ? "Account Access" : "Sign In / Sign Up"}
          subtitle={
            firebaseEnabled
              ? "Cloud sync is active."
              : "Firebase is not configured. Authentication runs in local demo mode."
          }
          onClose={closeOverlay}
          maxWidthClassName="max-w-5xl"
        >
          <div className="space-y-4">
            <div className="vw-auth-tabs" role="tablist" aria-label="Authentication mode">
              <button
                type="button"
                role="tab"
                aria-selected={authMode === "signin"}
                className={`vw-auth-tab ${authMode === "signin" ? "vw-auth-tab-active" : ""}`}
                onClick={() => {
                  setAuthMode("signin");
                  setAuthError(null);
                  setAuthNotice(null);
                }}
              >
                Sign In
              </button>
              <button
                type="button"
                role="tab"
                aria-selected={authMode === "signup"}
                className={`vw-auth-tab ${authMode === "signup" ? "vw-auth-tab-active" : ""}`}
                onClick={() => {
                  setAuthMode("signup");
                  setAuthError(null);
                  setAuthNotice(null);
                }}
              >
                Sign Up
              </button>
            </div>

            <div className="space-y-3">
              <form className="vw-auth-form rounded-2xl border border-[#E4E4E7] bg-white px-4 py-4" onSubmit={handleAuthSubmit}>
                {authMode === "signup" && (
                  <div className="grid gap-3 md:grid-cols-2">
                    <label className="text-sm text-[#09090B]">
                      <span className="block text-xs text-[#71717A]">Name</span>
                      <input
                        className="vw-auth-input mt-1 w-full rounded-xl border border-[#E4E4E7] bg-white px-3 py-2 text-sm text-[#09090B]"
                        value={authName}
                        onChange={(event) => setAuthName(event.target.value)}
                        placeholder="Alex Rivera"
                      />
                    </label>
                    <label className="text-sm text-[#09090B]">
                      <span className="block text-xs text-[#71717A]">Workspace Role</span>
                      <input
                        className="vw-auth-input mt-1 w-full rounded-xl border border-[#E4E4E7] bg-white px-3 py-2 text-sm text-[#09090B]"
                        value={authWorkspaceRole}
                        onChange={(event) => setAuthWorkspaceRole(event.target.value)}
                        placeholder="Engineering"
                      />
                    </label>
                  </div>
                )}

                <div className="mt-3 grid gap-3 md:grid-cols-2">
                  <label className="text-sm text-[#09090B]">
                    <span className="block text-xs text-[#71717A]">Email</span>
                    <input
                      className="vw-auth-input mt-1 w-full rounded-xl border border-[#E4E4E7] bg-white px-3 py-2 text-sm text-[#09090B]"
                      value={authEmail}
                      onChange={(event) => setAuthEmail(event.target.value)}
                      placeholder="you@voicewave.app"
                      type="email"
                      required
                    />
                  </label>
                  <label className="text-sm text-[#09090B]">
                    <span className="block text-xs text-[#71717A]">Password</span>
                    <input
                      className="vw-auth-input mt-1 w-full rounded-xl border border-[#E4E4E7] bg-white px-3 py-2 text-sm text-[#09090B]"
                      value={authPassword}
                      onChange={(event) => setAuthPassword(event.target.value)}
                      placeholder="********"
                      type={authShowPassword ? "text" : "password"}
                      required
                    />
                  </label>
                </div>

                {authMode === "signup" && (
                  <div className="mt-3">
                    <label className="text-sm text-[#09090B]">
                      <span className="block text-xs text-[#71717A]">Confirm Password</span>
                      <input
                        className="vw-auth-input mt-1 w-full rounded-xl border border-[#E4E4E7] bg-white px-3 py-2 text-sm text-[#09090B]"
                        value={authConfirmPassword}
                        onChange={(event) => setAuthConfirmPassword(event.target.value)}
                        placeholder="********"
                        type={authShowConfirmPassword ? "text" : "password"}
                        required
                      />
                    </label>
                  </div>
                )}

                <div className="mt-3 flex flex-wrap items-center gap-2 text-xs">
                  <button
                    type="button"
                    className="vw-auth-link"
                    onClick={() => setAuthShowPassword((current) => !current)}
                  >
                    {authShowPassword ? "Hide Password" : "Show Password"}
                  </button>
                  {authMode === "signup" ? (
                    <button
                      type="button"
                      className="vw-auth-link"
                      onClick={() => setAuthShowConfirmPassword((current) => !current)}
                    >
                      {authShowConfirmPassword ? "Hide Confirm" : "Show Confirm"}
                    </button>
                  ) : (
                    <button type="button" className="vw-auth-link" onClick={() => void handleForgotPassword()}>
                      Forgot Password?
                    </button>
                  )}
                </div>

                <label className="mt-3 inline-flex items-center gap-2 text-xs text-[#52525B]">
                  <input
                    type="checkbox"
                    checked={authRememberMe}
                    onChange={(event) => setAuthRememberMe(event.target.checked)}
                  />
                  Keep me signed in
                </label>

                <div className="mt-4 flex flex-wrap gap-2">
                  <button type="submit" className="vw-btn-primary" disabled={authPending}>
                    {authPending ? "Please wait..." : authMode === "signin" ? "Sign In" : "Create Account"}
                  </button>
                  <button type="button" className="vw-btn-secondary" onClick={continueAsGuest}>
                    Continue as Guest
                  </button>
                </div>
              </form>

              {authNotice && (
                <section className="rounded-2xl border border-[#BFDBFE] bg-[#EFF6FF] px-4 py-3 text-sm text-[#1E40AF]">
                  {authNotice}
                </section>
              )}

              {authError && (
                <section className="rounded-2xl border border-[#FED7D7] bg-[#FFF5F5] px-4 py-3 text-sm text-[#9B2C2C]">
                  {authError}
                </section>
              )}

              {isDemoAuthenticated && demoProfile && (
                <section className="rounded-2xl border border-[#E4E4E7] bg-[#FAFAFA] px-4 py-3 text-sm text-[#3F3F46]">
                  <p>
                    Signed in as <span className="font-semibold text-[#09090B]">{demoProfile.email}</span>
                    {cloudUserId ? " with cloud sync enabled." : " on local demo mode."}
                  </p>
                  <div className="mt-3 flex flex-wrap gap-2">
                    <button
                      type="button"
                      className="vw-btn-secondary"
                      onClick={() => void handleSignOut()}
                      disabled={authPending}
                    >
                      {authPending ? "Signing Out..." : "Sign Out"}
                    </button>
                    <button
                      type="button"
                      className="vw-btn-secondary"
                      onClick={() => {
                        setAuthMode("signin");
                        setAuthError(null);
                        setAuthNotice(null);
                      }}
                    >
                      Switch Account
                    </button>
                  </div>
                </section>
              )}
            </div>
          </div>
        </OverlayModal>
      )}

      {activeOverlay === "style" && (
        <OverlayModal
          title="Style"
          subtitle="Visual and writing preferences for your workspace."
          onClose={closeOverlay}
        >
          <div className="space-y-4">
            <section className="vw-surface-base rounded-2xl border border-[#E4E4E7] bg-white px-4 py-4">
              <div className="flex items-start gap-3">
                <div className="mt-0.5 rounded-full bg-[#F4F4F5] p-2 text-[#18181B]">
                  <Palette size={16} />
                </div>
                <div>
                  <p className="text-sm font-semibold text-[#09090B]">Style Presets</p>
                  <p className="mt-1 text-sm text-[#71717A]">
                    Style customization is queued for a dedicated pass. The current theme is already
                    locked to match the production baseline.
                  </p>
                </div>
              </div>
            </section>
            <section className="vw-surface-elevated rounded-2xl border border-[#E4E4E7] bg-[#FAFAFA] px-4 py-4">
              <p className="text-sm font-semibold text-[#09090B]">Current Theme</p>
              <p className="mt-1 text-xs text-[#71717A]">
                Harmonic v1.0 with high-contrast cards, neutral white surfaces, and focused action styling.
              </p>
            </section>
          </div>
        </OverlayModal>
      )}

      {activeOverlay === "help" && (
        <OverlayModal
          title="Help"
          subtitle="Quick guidance for everyday dictation reliability."
          onClose={closeOverlay}
        >
          <div className="space-y-4">
            <section className="vw-surface-base rounded-2xl border border-[#E4E4E7] bg-white px-4 py-4">
              <div className="flex items-start gap-3">
                <div className="mt-0.5 rounded-full bg-[#F4F4F5] p-2 text-[#18181B]">
                  <CircleHelp size={16} />
                </div>
                <div>
                  <p className="text-sm font-semibold text-[#09090B]">Push-to-talk Best Practice</p>
                  <p className="mt-1 text-sm text-[#71717A]">
                    Hold the key first, then speak naturally, then release to transcribe.
                  </p>
                </div>
              </div>
            </section>
            <section className="vw-surface-base rounded-2xl border border-[#E4E4E7] bg-white px-4 py-4">
              <p className="text-sm font-semibold text-[#09090B]">Troubleshooting Flow</p>
              <ul className="mt-2 space-y-1 text-sm text-[#71717A]">
                <li>1. Refresh microphone devices.</li>
                <li>2. Switch away from headset hands-free profiles.</li>
                <li>3. Run the 10s audio quality check in Settings Advanced.</li>
              </ul>
            </section>
            <section className="vw-surface-base rounded-2xl border border-[#E4E4E7] bg-white px-4 py-4">
              <div className="flex items-center justify-between gap-3">
                <div>
                  <p className="text-sm font-semibold text-[#09090B]">First-run setup</p>
                  <p className="mt-1 text-sm text-[#71717A]">
                    Replay the guided mic check and dictation rehearsal.
                  </p>
                </div>
                <button
                  type="button"
                  className="vw-btn-secondary vw-btn-sm"
                  onClick={() => {
                    closeOverlay();
                    void restartOnboarding();
                  }}
                >
                  Run setup again
                </button>
              </div>
            </section>
          </div>
        </OverlayModal>
      )}

      {showOnboarding && (
        <Onboarding
          catalog={setupCatalog}
          statuses={modelStatuses}
          hasInstalledModel={hasInstalledModel}
          installModel={installModel}
          makeModelActive={makeModelActive}
          hotkeyLabel={settings.pushToTalkHotkey}
          snapshot={snapshot}
          onComplete={() => void completeOnboarding()}
        />
      )}

      {showModelSetupGate && (
        <div className="vw-model-gate-backdrop">
          <section className="vw-model-gate-card" role="dialog" aria-modal="true" aria-label="Enable Dictation">
            <header>
              <h3 className="vw-section-heading text-2xl font-semibold text-[#09090B]">Enable Dictation</h3>
              <p className="mt-1 text-sm text-[#64748B]">Install a model to start transcription.</p>
            </header>

            <div className="vw-model-gate-tabs" role="tablist" aria-label="Model size selector">
              <button
                type="button"
                role="tab"
                aria-selected={setupModelChoice === "fw-small.en"}
                className={`vw-model-gate-tab ${setupModelChoice === "fw-small.en" ? "vw-model-gate-tab-active" : ""}`}
                onClick={() => setSetupModelChoice("fw-small.en")}
                disabled={setupModelPending}
              >
                <span className="vw-model-gate-badge">Recommended</span>
                <span className="vw-model-gate-tab-title">Small</span>
                <span className="vw-model-gate-tab-copy">Fast setup. Best for most devices.</span>
              </button>

              <button
                type="button"
                role="tab"
                aria-selected={setupModelChoice === "fw-large-v3-turbo"}
                className={`vw-model-gate-tab ${setupModelChoice === "fw-large-v3-turbo" ? "vw-model-gate-tab-active" : ""}`}
                onClick={() => setSetupModelChoice("fw-large-v3-turbo")}
                disabled={setupModelPending}
              >
                <span className="vw-model-gate-tab-title">Large Turbo</span>
                <span className="vw-model-gate-tab-copy">
                  Higher quality. Use only on high-power devices with strong GPU.
                </span>
              </button>
            </div>

            <div className="vw-model-gate-meta">
              <span className="vw-chip">{selectedSetupCatalogRow?.displayName ?? setupModelChoice}</span>
              {selectedSetupCatalogRow && (
                <span className="vw-chip">{formatBytes(selectedSetupCatalogRow.sizeBytes)}</span>
              )}
            </div>

            <div className="mt-4 flex flex-wrap items-center gap-2">
              <button
                type="button"
                className="vw-btn-primary"
                onClick={() => void handleSetupModelInstall()}
                disabled={setupModelPending || selectedSetupStatus?.state === "downloading"}
              >
                {setupModelPending
                  ? "Starting Download..."
                  : selectedSetupStatus?.state === "downloading"
                    ? `Downloading ${selectedSetupStatus.progress}%`
                    : setupModelChoice === "fw-small.en"
                      ? "Download Small Model"
                      : "Download Large Model"}
              </button>
            </div>

            {(setupModelError || selectedSetupStatus?.message || displayError) && (
              <section className="mt-4 rounded-2xl border border-[#E4E4E7] bg-[#FAFAFA] px-4 py-3 text-sm text-[#3F3F46]">
                {setupModelError ?? selectedSetupStatus?.message ?? displayError}
              </section>
            )}
          </section>
        </div>
      )}

    </>
  );
}

export default App;


