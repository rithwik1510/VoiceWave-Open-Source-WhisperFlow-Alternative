import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import * as hookModule from "./hooks/useVoiceWave";

function buildHookMock(overrides: Record<string, unknown> = {}) {
  return {
    activeState: "idle",
    approveDictionaryQueueEntry: vi.fn(),
    benchmarkResults: null,
    cancelModelInstall: vi.fn(),
    clearSessionHistory: vi.fn(),
    entitlement: {
      tier: "free",
      status: "free",
      isPro: false,
      isOwnerOverride: false,
      expiresAtUtcMs: null,
      graceUntilUtcMs: null,
      lastRefreshedAtUtcMs: 0,
      plan: {
        basePriceUsdMonthly: 4,
        launchPriceUsdMonthly: 1.5,
        launchMonths: 3,
        displayBasePrice: "$4/mo",
        displayLaunchPrice: "$1.50/mo",
        offerCopy: "Launch offer: first 3 months at $1.50, then $4/month"
      },
      message: null
    },
    diagnosticsStatus: {
      optIn: false,
      recordCount: 0,
      lastExportPath: null,
      lastExportedAtUtcMs: null,
      watchdogRecoveryCount: 0
    },
    deleteDictionaryTerm: vi.fn(),
    dictionaryQueue: [],
    dictionaryTerms: [],
    error: null,
    exportHistoryPreset: vi.fn(),
    exportDiagnosticsBundle: vi.fn(),
    historyPolicy: "days30",
    isPro: false,
    isOwnerOverride: false,
    hotkeys: {
      config: { toggle: "Ctrl+Alt+X", pushToTalk: "Ctrl+Windows" },
      conflicts: [],
      registrationSupported: true,
      registrationError: null
    },
    inputDevices: [],
    insertFinalTranscript: vi.fn(),
    installModel: vi.fn(),
    installedModels: [],
    lastHotkeyEvent: null,
    lastInsertion: null,
    makeModelActive: vi.fn(),
    modelCatalog: [],
    modelRecommendation: null,
    modelSpeeds: {},
    modelStatuses: {},
    lastHistoryExport: null,
    lastDiagnosticsExport: null,
    lastLatency: null,
    openBillingPortal: vi.fn(),
    permissions: { microphone: "granted", insertionCapability: "available", message: null },
    proRequiredFeature: null,
    micLevel: 0,
    micLevelError: null,
    audioQualityReport: null,
    micQualityWarning: null,
    pauseModelInstall: vi.fn(),
    pruneHistory: vi.fn(),
    recentInsertions: [],
    refreshPhase3Data: vi.fn(),
    refreshInputDevices: vi.fn(),
    resumeModelInstall: vi.fn(),
    rejectDictionaryQueueEntry: vi.fn(),
    requestMicAccess: vi.fn(),
    runAudioQualityDiagnostic: vi.fn(),
    runBenchmarkAndRecommend: vi.fn(),
    runDictation: vi.fn(),
    searchHistory: vi.fn(),
    sessionHistory: [],
    setAppProfiles: vi.fn(),
    setCodeModeSettings: vi.fn(),
    setDictationProfile: vi.fn(),
    setLlmPolishEnabled: vi.fn().mockResolvedValue(true),
    polishModelProgress: null,
    setDiagnosticsOptIn: vi.fn(),
    setDomainPacks: vi.fn(),
    setFormatProfile: vi.fn(),
    setInputDevice: vi.fn(),
    setMaxUtteranceMs: vi.fn(),
    setOwnerOverride: vi.fn(),
    setReleaseTailMs: vi.fn(),
    setDecodeMode: vi.fn(),
    setProPostProcessingEnabled: vi.fn(),
    setSessionStarred: vi.fn(),
    setPreferClipboardFallback: vi.fn(),
    setSpokenEditCommands: vi.fn(),
    completeOnboarding: vi.fn(),
    restartOnboarding: vi.fn(),
    setVadThreshold: vi.fn(),
    addSessionTag: vi.fn(),
    addDictionaryTerm: vi.fn(),
    exportDictionary: vi.fn(),
    importDictionary: vi.fn(),
    refreshDictionary: vi.fn(),
    syncDictionaryWithCloud: vi.fn().mockResolvedValue([]),
    voiceSnippets: [],
    addVoiceSnippet: vi.fn(),
    updateVoiceSnippet: vi.fn(),
    deleteVoiceSnippet: vi.fn(),
    refreshVoiceSnippets: vi.fn(),
    syncVoiceSnippetsWithCloud: vi.fn().mockResolvedValue({
      snippets: [],
      records: [],
      limitExceeded: false
    }),
    resetVadThreshold: vi.fn(),
    restorePurchase: vi.fn(),
    startProCheckout: vi.fn(),
    settings: {
      inputDevice: null,
      activeModel: "fw-small.en",
      showFloatingHud: false,
      vadThreshold: 0.014,
      maxUtteranceMs: 30000,
      releaseTailMs: 350,
      decodeMode: "balanced",
      diagnosticsOptIn: false,
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
      llmPolishEnabled: false,
      spokenEditCommands: true,
      onboardingCompleted: true
    },
    switchToRecommendedInput: vi.fn(),
    recommendedVadThreshold: 0.014,
    snapshot: {
      state: "idle",
      lastPartial: null,
      lastFinal: null,
      activeModel: "fw-small.en"
    },
    stopDictation: vi.fn(),
    tauriAvailable: false,
    undoInsertion: vi.fn(),
    updateHotkeys: vi.fn(),
    refreshEntitlement: vi.fn(),
    updateRetentionPolicy: vi.fn(),
    ...overrides
  };
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("App navigation and phase three panels", () => {
  it("shows Pro Tools navigation by default during the release offer", async () => {
    render(<App />);
    expect(screen.getByRole("button", { name: "Pro Tools" })).toBeInTheDocument();
  });

  it("shows Polish Profiles for pro users and selects Coding with one atomic call", async () => {
    const setDictationProfile = vi.fn().mockResolvedValue(undefined);
    const setLlmPolishEnabled = vi.fn().mockResolvedValue(true);
    const setFormatProfile = vi.fn();
    const useVoiceWaveSpy = vi
      .spyOn(hookModule, "useVoiceWave")
      .mockReturnValue(
        buildHookMock({
          isPro: true,
          entitlement: {
            tier: "pro",
            status: "pro_active",
            isPro: true,
            isOwnerOverride: false,
            expiresAtUtcMs: null,
            graceUntilUtcMs: null,
            lastRefreshedAtUtcMs: 0,
            plan: {
              basePriceUsdMonthly: 4,
              launchPriceUsdMonthly: 1.5,
              launchMonths: 3,
              displayBasePrice: "$4/mo",
              displayLaunchPrice: "$1.50/mo",
              offerCopy: "Launch offer: first 3 months at $1.50, then $4/month"
            },
            message: null
          },
          setDictationProfile,
          setLlmPolishEnabled,
          setFormatProfile
        }) as any
      );

    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Pro Tools" }));
    expect(screen.getByText("Polish Profiles")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Standard/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Coding/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Writing/ })).toBeInTheDocument();
    // Casual is cut from the v1 lineup (plan 010 Rev 3: failed the
    // distinctness gate against Writing) — it must NOT render as a card.
    expect(screen.queryByRole("button", { name: /^Casual/ })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Literal/ })).toBeInTheDocument();
    // The wait-validated profiles disclose the latency trade up front.
    expect(screen.getAllByText("Typically adds ~2s once the local model is warm.").length).toBe(2);
    expect(
      screen.getByText("No AI rewriting — your words as recognized, punctuation only.")
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /^Coding/ }));
    await waitFor(() => {
      // ONE atomic profile write — never the old five sequential writes.
      expect(setDictationProfile).toHaveBeenCalledWith("coding");
      expect(setLlmPolishEnabled).toHaveBeenCalledWith(true);
      expect(setFormatProfile).not.toHaveBeenCalled();
    });

    useVoiceWaveSpy.mockRestore();
  });

  it("falls back to the deprecated multi-write path when the backend lacks set_dictation_profile", async () => {
    const setDictationProfile = vi
      .fn()
      .mockRejectedValue(new Error("Command set_dictation_profile not found"));
    const setFormatProfile = vi.fn();
    const setDomainPacks = vi.fn();
    const setCodeModeSettings = vi.fn();
    const setAppProfiles = vi.fn();
    const setProPostProcessingEnabled = vi.fn();
    const useVoiceWaveSpy = vi
      .spyOn(hookModule, "useVoiceWave")
      .mockReturnValue(
        buildHookMock({
          isPro: true,
          setDictationProfile,
          setFormatProfile,
          setDomainPacks,
          setCodeModeSettings,
          setAppProfiles,
          setProPostProcessingEnabled
        }) as any
      );

    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Pro Tools" }));
    fireEvent.click(screen.getByRole("button", { name: /^Coding/ }));

    await waitFor(() => {
      expect(setDictationProfile).toHaveBeenCalledWith("coding");
      expect(setFormatProfile).toHaveBeenCalledWith("code-doc");
      expect(setDomainPacks).toHaveBeenCalledWith(["coding"]);
      expect(setCodeModeSettings).toHaveBeenCalledWith(
        expect.objectContaining({ enabled: true, preferredCasing: "camelCase" })
      );
      expect(setAppProfiles).toHaveBeenCalledWith(
        expect.objectContaining({ activeTarget: "editor" })
      );
      expect(setProPostProcessingEnabled).toHaveBeenCalledWith(true);
    });

    useVoiceWaveSpy.mockRestore();
  });

  it("switches between home, models, dictionary, and snippets tabs", async () => {
    render(<App />);

    expect(screen.getByText("Good morning, Rishi.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Run 10s Check" })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Models" }));
    expect(screen.getByText("Model Manager")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Dictionary" }));
    expect(screen.getByText("Personal Dictionary")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Snippets" }));
    expect(screen.getByText("Voice Snippets")).toBeInTheDocument();
    expect(screen.getByText("Create your first voice snippet")).toBeInTheDocument();
  });

  it("saves a multiline snippet through the local action", async () => {
    const addVoiceSnippet = vi.fn().mockResolvedValue(undefined);
    const useVoiceWaveSpy = vi.spyOn(hookModule, "useVoiceWave").mockReturnValue(
      buildHookMock({ addVoiceSnippet }) as any
    );

    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Snippets" }));
    fireEvent.click(screen.getByRole("button", { name: "New snippet" }));
    fireEvent.change(screen.getByLabelText("Spoken trigger"), { target: { value: "my reply" } });
    fireEvent.change(screen.getByLabelText("Exact expansion"), {
      target: { value: "Hello,\n\nThanks for reaching out." }
    });
    fireEvent.click(screen.getByRole("button", { name: "Save snippet" }));

    await waitFor(() => {
      expect(addVoiceSnippet).toHaveBeenCalledWith("my reply", "Hello,\n\nThanks for reaching out.");
    });
    useVoiceWaveSpy.mockRestore();
  });

  it("edits and confirms deletion of an existing snippet", async () => {
    const updateVoiceSnippet = vi.fn().mockResolvedValue(undefined);
    const deleteVoiceSnippet = vi.fn().mockResolvedValue(undefined);
    const useVoiceWaveSpy = vi.spyOn(hookModule, "useVoiceWave").mockReturnValue(
      buildHookMock({
        voiceSnippets: [{
          snippetId: "snippet-1",
          trigger: "my email",
          normalizedTrigger: "my email",
          expansion: "name@example.com",
          createdAtUtcMs: 1,
          updatedAtUtcMs: 1
        }],
        updateVoiceSnippet,
        deleteVoiceSnippet
      }) as any
    );

    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Snippets" }));
    fireEvent.click(screen.getByRole("button", { name: "Edit my email" }));
    fireEvent.change(screen.getByLabelText("Exact expansion"), { target: { value: "new@example.com" } });
    fireEvent.click(screen.getByRole("button", { name: "Save snippet" }));
    await waitFor(() => {
      expect(updateVoiceSnippet).toHaveBeenCalledWith("snippet-1", "my email", "new@example.com");
    });

    fireEvent.click(screen.getByRole("button", { name: "Delete my email" }));
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(deleteVoiceSnippet).toHaveBeenCalledWith("snippet-1"));
    useVoiceWaveSpy.mockRestore();
  });

  it("searches snippets and scopes keyboard creation and validation to the page", async () => {
    const useVoiceWaveSpy = vi.spyOn(hookModule, "useVoiceWave").mockReturnValue(
      buildHookMock({
        voiceSnippets: [
          {
            snippetId: "snippet-support",
            trigger: "support reply",
            normalizedTrigger: "support reply",
            expansion: "Hello from support",
            createdAtUtcMs: 1,
            updatedAtUtcMs: 1
          },
          {
            snippetId: "snippet-email",
            trigger: "my email",
            normalizedTrigger: "my email",
            expansion: "name@example.com",
            createdAtUtcMs: 2,
            updatedAtUtcMs: 2
          }
        ]
      }) as any
    );

    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Snippets" }));
    const search = screen.getByRole("textbox", { name: "Search voice snippets" });
    fireEvent.keyDown(document, { key: "f", ctrlKey: true });
    expect(search).toHaveFocus();
    fireEvent.change(search, { target: { value: "support" } });
    expect(screen.getByText("support reply")).toBeInTheDocument();
    expect(screen.queryByText("my email")).not.toBeInTheDocument();

    fireEvent.keyDown(document, { key: "n", ctrlKey: true });
    expect(screen.getByLabelText("Spoken trigger")).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Exact expansion"), { target: { value: "value" } });
    fireEvent.click(screen.getByRole("button", { name: "Save snippet" }));
    expect(screen.getByText("Use a spoken trigger between 1 and 60 characters.")).toBeInTheDocument();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByLabelText("Spoken trigger")).not.toBeInTheDocument();
    useVoiceWaveSpy.mockRestore();
  });

  it("searches the local approved dictionary by term and source", async () => {
    const useVoiceWaveSpy = vi.spyOn(hookModule, "useVoiceWave").mockReturnValue(
      buildHookMock({
        dictionaryTerms: [
          { termId: "dt-1", term: "Kubernetes", source: "manual-add", createdAtUtcMs: 2 },
          { termId: "dt-2", term: "Tauri", source: "queue-approval", createdAtUtcMs: 1 }
        ]
      }) as any
    );

    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Dictionary" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Search approved dictionary terms" }), {
      target: { value: "queue" }
    });

    expect(screen.getByText("Tauri")).toBeInTheDocument();
    expect(screen.queryByText("Kubernetes")).not.toBeInTheDocument();
    useVoiceWaveSpy.mockRestore();
  });

  it("approves the edited pending value through the local dictionary action", async () => {
    const approveDictionaryQueueEntry = vi.fn().mockResolvedValue(undefined);
    const useVoiceWaveSpy = vi.spyOn(hookModule, "useVoiceWave").mockReturnValue(
      buildHookMock({
        dictionaryQueue: [{
          entryId: "dq-1",
          term: "Voice Wave",
          sourcePreview: "voice wave desktop",
          createdAtUtcMs: 1
        }],
        approveDictionaryQueueEntry
      }) as any
    );

    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Dictionary" }));
    fireEvent.click(screen.getByRole("button", { name: /Pending review queue/ }));
    const edit = screen.getByRole("textbox", { name: "Edit pending term Voice Wave" });
    fireEvent.change(edit, { target: { value: "VoiceWave" } });
    fireEvent.click(screen.getByRole("button", { name: "Approve" }));

    await waitFor(() => {
      expect(approveDictionaryQueueEntry).toHaveBeenCalledWith("dq-1", "VoiceWave");
    });
    useVoiceWaveSpy.mockRestore();
  });

  it("retains an invalid pending edit when local approval rejects it", async () => {
    const approveDictionaryQueueEntry = vi.fn().mockRejectedValue(new Error("dictionary term is empty"));
    const useVoiceWaveSpy = vi.spyOn(hookModule, "useVoiceWave").mockReturnValue(
      buildHookMock({
        dictionaryQueue: [{
          entryId: "dq-invalid",
          term: "VoiceWave",
          sourcePreview: "preview",
          createdAtUtcMs: 1
        }],
        approveDictionaryQueueEntry
      }) as any
    );

    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Dictionary" }));
    fireEvent.click(screen.getByRole("button", { name: /Pending review queue/ }));
    const edit = screen.getByRole("textbox", { name: "Edit pending term VoiceWave" });
    fireEvent.change(edit, { target: { value: "" } });
    fireEvent.click(screen.getByRole("button", { name: "Approve" }));

    await waitFor(() => expect(approveDictionaryQueueEntry).toHaveBeenCalledWith("dq-invalid", ""));
    expect(screen.getByRole("textbox", { name: "Edit pending term VoiceWave" })).toHaveValue("");
    expect(screen.getByText("preview")).toBeInTheDocument();
    useVoiceWaveSpy.mockRestore();
  });

  it("opens the stats page from the nav", async () => {
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "Stats" }));
    // Web/test runtime has no Tauri backend, so the section explains itself.
    expect(screen.getByText("Stats are computed inside the desktop app.")).toBeInTheDocument();
  });

  it("opens the history page from the nav with retention controls and copyable records", async () => {
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "History" }));
    expect(screen.getByRole("group", { name: "History retention policy" })).toBeInTheDocument();
    // Web fallback mode seeds one demo record; it must render with a copy action.
    expect(
      screen.getByText("Phase three panel wiring is ready for desktop integration.")
    ).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Copy transcript" }).length).toBeGreaterThan(0);
  });

  it("supports model install action in web fallback mode", async () => {
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "Models" }));
    fireEvent.click(screen.getAllByRole("button", { name: "Install" })[0]);

    expect(
      screen.getByText("Desktop runtime is required to download models. Run npm run tauri:dev.")
    ).toBeInTheDocument();
  });

  it("renders long final transcript content without truncating the payload", async () => {
    const longFinalTranscript = Array.from({ length: 120 }, (_, idx) => `token-${idx}`).join(" ");
    const useVoiceWaveSpy = vi
      .spyOn(hookModule, "useVoiceWave")
      .mockReturnValue(
        buildHookMock({
          snapshot: {
            state: "inserted",
            lastPartial: null,
            lastFinal: longFinalTranscript,
            activeModel: "fw-small.en"
          }
        }) as any
      );

    render(<App />);
    expect(screen.getAllByText(longFinalTranscript).length).toBeGreaterThan(0);

    useVoiceWaveSpy.mockRestore();
  });

  it("renders diagnostics controls in settings and triggers opt-in and export actions", async () => {
    const setDiagnosticsOptIn = vi.fn();
    const exportDiagnosticsBundle = vi.fn();
    const useVoiceWaveSpy = vi
      .spyOn(hookModule, "useVoiceWave")
      .mockReturnValue(
        buildHookMock({
          tauriAvailable: true,
          diagnosticsStatus: {
            optIn: false,
            recordCount: 4,
            lastExportPath: null,
            lastExportedAtUtcMs: null,
            watchdogRecoveryCount: 1
          },
          setDiagnosticsOptIn,
          exportDiagnosticsBundle
        }) as any
      );

    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Settings" }));

    const settingsDialog = screen.getByRole("dialog", { name: "Settings" });
    expect(within(settingsDialog).getByText("Diagnostics")).toBeInTheDocument();
    // Diagnostics controls live in their own section of the settings rail.
    fireEvent.click(within(settingsDialog).getByRole("button", { name: "Diagnostics" }));
    fireEvent.click(within(settingsDialog).getByRole("checkbox", { name: "Enable diagnostics" }));
    expect(setDiagnosticsOptIn).toHaveBeenCalledWith(true);

    fireEvent.click(within(settingsDialog).getByRole("button", { name: "Export Diagnostics Bundle" }));
    expect(exportDiagnosticsBundle).toHaveBeenCalledTimes(1);

    useVoiceWaveSpy.mockRestore();
  });

  it("keeps advanced settings collapsed until explicitly expanded", async () => {
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Settings" }));

    const settingsDialog = screen.getByRole("dialog", { name: "Settings" });
    expect(within(settingsDialog).getByText("Advanced")).toBeInTheDocument();
    expect(within(settingsDialog).queryByText("Release Tail (ms)")).not.toBeInTheDocument();

    fireEvent.click(within(settingsDialog).getByRole("button", { name: /advanced/i }));
    expect(within(settingsDialog).getByText("Release Tail (ms)")).toBeInTheDocument();
  });

  it("shows the updates section with a check-for-updates action", async () => {
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Settings" }));

    const settingsDialog = screen.getByRole("dialog", { name: "Settings" });
    fireEvent.click(within(settingsDialog).getByRole("button", { name: "Updates" }));

    expect(within(settingsDialog).getByText("Current version")).toBeInTheDocument();
    // Outside the Tauri desktop runtime the check button is disabled with an
    // explanatory description instead of firing a doomed network request.
    const checkButton = within(settingsDialog).getByRole("button", { name: "Check now" });
    expect(checkButton).toBeDisabled();
    expect(within(settingsDialog).getByText("Update checks run in the desktop app.")).toBeInTheDocument();
  });

  it("opens style and help as separate popups", async () => {
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "Style" }));
    expect(screen.getByRole("dialog", { name: "Style" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Close Style" }));
    fireEvent.click(screen.getByRole("button", { name: "Help" }));
    expect(screen.getByRole("dialog", { name: "Help" })).toBeInTheDocument();
  });

  it("opens profile and auth overlays from the workspace menu while keeping guest access available", async () => {
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "Open workspace menu" }));
    const workspaceMenu = screen.getByRole("menu", { name: "Workspace menu" });
    fireEvent.click(within(workspaceMenu).getByRole("menuitem", { name: "Profile" }));

    const profileDialog = screen.getByRole("dialog", { name: "Profile" });
    expect(within(profileDialog).getByText("Guest Workspace")).toBeInTheDocument();

    fireEvent.click(within(profileDialog).getByRole("button", { name: "Sign In / Sign Up" }));
    const authDialog = screen.getByRole("dialog", { name: "Sign In / Sign Up" });
    expect(within(authDialog).getByRole("button", { name: "Continue as Guest" })).toBeInTheDocument();

    fireEvent.click(within(authDialog).getByRole("button", { name: "Continue as Guest" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: "Sign In / Sign Up" })).not.toBeInTheDocument();
    });
  });

  it("applies demo sign-in locally and reflects account details in profile", async () => {
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "Open workspace menu" }));
    const workspaceMenu = screen.getByRole("menu", { name: "Workspace menu" });
    fireEvent.click(within(workspaceMenu).getByRole("menuitem", { name: "Sign In" }));

    const authDialog = screen.getByRole("dialog", { name: "Sign In / Sign Up" });
    fireEvent.change(within(authDialog).getByLabelText("Email"), { target: { value: "alex@voicewave.app" } });
    fireEvent.change(within(authDialog).getByLabelText("Password"), { target: { value: "pass-1234" } });
    fireEvent.click(within(authDialog).getByRole("button", { name: "Sign In" }));

    await waitFor(() => {
      const profileDialog = screen.getByRole("dialog", { name: "Profile" });
      expect(within(profileDialog).getByText("alex@voicewave.app")).toBeInTheDocument();
    });
  });

  it("collapses and expands the sidebar shell", async () => {
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "Collapse sidebar" }));
    expect(screen.getByRole("button", { name: "Expand sidebar" })).toBeInTheDocument();
  });
});
