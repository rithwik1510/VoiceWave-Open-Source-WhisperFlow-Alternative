import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as tauri from "./tauri";

const coreMocks = vi.hoisted(() => ({
  invoke: vi.fn()
}));

const eventMocks = vi.hoisted(() => ({
  listen: vi.fn()
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: coreMocks.invoke
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: eventMocks.listen
}));

function enableTauriRuntimeFlag(): void {
  Object.defineProperty(window, "__TAURI_INTERNALS__", {
    configurable: true,
    value: {}
  });
}

function disableTauriRuntimeFlag(): void {
  const win = window as unknown as { __TAURI_INTERNALS__?: unknown };
  delete win.__TAURI_INTERNALS__;
}

describe("tauri bridge", () => {
  beforeEach(() => {
    coreMocks.invoke.mockReset();
    eventMocks.listen.mockReset();
    coreMocks.invoke.mockResolvedValue(undefined);
    eventMocks.listen.mockResolvedValue(() => undefined);
    disableTauriRuntimeFlag();
  });

  afterEach(() => {
    disableTauriRuntimeFlag();
  });

  it("detects when tauri runtime is unavailable", () => {
    expect(tauri.canUseTauri()).toBe(false);
  });

  it("rejects invoke when runtime is unavailable", async () => {
    await expect(tauri.invokeVoicewave("get_voicewave_snapshot")).rejects.toThrow(
      "Tauri runtime is not available."
    );
    expect(coreMocks.invoke).not.toHaveBeenCalled();
  });

  it("invokes commands when runtime is available", async () => {
    enableTauriRuntimeFlag();
    coreMocks.invoke.mockResolvedValueOnce({ ok: true });

    const result = await tauri.invokeVoicewave<{ ok: boolean }>("demo-command", { a: 1 });

    expect(result.ok).toBe(true);
    expect(coreMocks.invoke).toHaveBeenCalledWith("demo-command", { a: 1 });
  });

  it("returns noop unlisten when runtime is unavailable", async () => {
    const unlisten = await tauri.listenVoicewaveState(() => undefined);
    unlisten();
    expect(eventMocks.listen).not.toHaveBeenCalled();
  });

  it("maps event payloads when runtime is available", async () => {
    enableTauriRuntimeFlag();
    const callback = vi.fn();
    eventMocks.listen.mockImplementationOnce(async (_eventName, handler) => {
      handler({ payload: { phase: "listening" } });
      return () => undefined;
    });

    const unlisten = await tauri.listenVoicewaveState(callback);

    expect(eventMocks.listen).toHaveBeenCalledTimes(1);
    expect(callback).toHaveBeenCalledWith({ phase: "listening" });
    expect(typeof unlisten).toBe("function");
  });

  it("forwards bridge helpers to expected tauri commands", async () => {
    enableTauriRuntimeFlag();

    await tauri.loadSnapshot();
    await tauri.loadSettings();
    await tauri.getEntitlementSnapshot();
    await tauri.startProCheckout();
    await tauri.refreshEntitlement();
    await tauri.restorePurchase();
    await tauri.openBillingPortal();
    await tauri.setOwnerDeviceOverride(true, "pw");
    await tauri.updateSettings({} as never);
    await tauri.setFormatProfile("meeting" as never);
    await tauri.setActiveDomainPacks(["general"] as never);
    await tauri.setAppProfileOverrides({} as never);
    await tauri.setCodeModeSettings({} as never);
    await tauri.setProPostProcessingEnabled(true);
    await tauri.getDiagnosticsStatus();
    await tauri.setDiagnosticsOptIn(true);
    await tauri.exportDiagnosticsBundle();
    await tauri.loadHotkeyConfig();
    await tauri.updateHotkeyConfig({} as never);
    await tauri.getPermissionSnapshot();
    await tauri.listInputDevices();
    await tauri.requestMicrophoneAccess();
    await tauri.startMicLevelMonitor();
    await tauri.stopMicLevelMonitor();
    await tauri.runAudioQualityDiagnostic(1200);
    await tauri.insertText({} as never);
    await tauri.undoLastInsertion();
    await tauri.getRecentInsertions(7);
    await tauri.startDictation("microphone", "handsFree");
    await tauri.cancelDictation();
    await tauri.stopDictation();
    await tauri.showMainWindow();
    await tauri.setPillReviewMode(true);
    await tauri.triggerHotkeyAction("toggle" as never, "down" as never);
    await tauri.listModelCatalog();
    await tauri.listInstalledModels();
    await tauri.getModelStatus("fw-small.en");
    await tauri.downloadModel({ modelId: "fw-small.en" });
    await tauri.cancelModelDownload("fw-small.en");
    await tauri.pauseModelDownload("fw-small.en");
    await tauri.resumeModelDownload("fw-small.en");
    await tauri.setActiveModel("fw-small.en");
    await tauri.runModelBenchmark({} as never);
    await tauri.getBenchmarkResults();
    await tauri.recommendModel({} as never);
    await tauri.getSessionHistory();
    await tauri.searchSessionHistory("hello", ["tag"], true);
    await tauri.tagSession("record-1", "qa");
    await tauri.toggleStarSession("record-1", true);
    await tauri.exportSessionHistoryPreset("plain" as never);
    await tauri.setHistoryRetention("days30" as never);
    await tauri.pruneHistoryNow();
    await tauri.clearHistory();
    await tauri.getDictionaryQueue(10);
    await tauri.approveDictionaryEntry("dq-1", "VoiceWave");
    await tauri.rejectDictionaryEntry("dq-1", "noise");
    await tauri.getDictionaryTerms("voice");
    await tauri.removeDictionaryTerm("dt-1");
    await tauri.addDictionaryTerm("VoiceWave");
    await tauri.exportDictionary();
    await tauri.importDictionary("{\"version\":1}");
    await tauri.getDictionarySyncRecords();
    await tauri.reconcileDictionaryRecords([{
      term: "VoiceWave",
      normalizedTerm: "voicewave",
      source: "manual-add",
      createdAtUtcMs: 1_700_000_000_000,
      updatedAtUtcMs: 1_700_000_000_000,
      deletedAtUtcMs: null
    }]);
    await tauri.listVoiceSnippets("reply");
    await tauri.addVoiceSnippet("my reply", "Hello!");
    await tauri.updateVoiceSnippet("vs-1", "support reply", "Hello there!");
    await tauri.removeVoiceSnippet("vs-1");
    await tauri.getVoiceSnippetSyncRecords();
    await tauri.reconcileVoiceSnippetRecords([{
      trigger: "Support reply",
      normalizedTrigger: "support reply",
      expansion: "Hello there!",
      createdAtUtcMs: 1_700_000_000_000,
      updatedAtUtcMs: 1_700_000_000_000,
      deletedAtUtcMs: null
    }]);

    expect(coreMocks.invoke).toHaveBeenCalledWith("get_voicewave_snapshot", undefined);
    expect(coreMocks.invoke).toHaveBeenCalledWith("set_owner_device_override", {
      enabled: true,
      passphrase: "pw"
    });
    expect(coreMocks.invoke).toHaveBeenCalledWith("run_audio_quality_diagnostic", { durationMs: 1200 });
    expect(coreMocks.invoke).toHaveBeenCalledWith("get_recent_insertions", { limit: 7 });
    expect(coreMocks.invoke).toHaveBeenCalledWith("start_dictation", {
      mode: "microphone",
      controlMode: "handsFree"
    });
    expect(coreMocks.invoke).toHaveBeenCalledWith("search_session_history", {
      query: "hello",
      tags: ["tag"],
      starred: true
    });
    expect(coreMocks.invoke).toHaveBeenCalledWith("approve_dictionary_entry", {
      entryId: "dq-1",
      normalizedText: "VoiceWave"
    });
    expect(coreMocks.invoke).toHaveBeenCalledWith("add_dictionary_term", { term: "VoiceWave" });
    expect(coreMocks.invoke).toHaveBeenCalledWith("export_dictionary", undefined);
    expect(coreMocks.invoke).toHaveBeenCalledWith("import_dictionary", {
      payload: "{\"version\":1}"
    });
    expect(coreMocks.invoke).toHaveBeenCalledWith("get_dictionary_sync_records", undefined);
    expect(coreMocks.invoke).toHaveBeenCalledWith("reconcile_dictionary_records", {
      records: [expect.objectContaining({ normalizedTerm: "voicewave" })]
    });
    expect(coreMocks.invoke).toHaveBeenCalledWith("list_voice_snippets", { query: "reply" });
    expect(coreMocks.invoke).toHaveBeenCalledWith("add_voice_snippet", {
      trigger: "my reply",
      expansion: "Hello!"
    });
    expect(coreMocks.invoke).toHaveBeenCalledWith("update_voice_snippet", {
      snippetId: "vs-1",
      trigger: "support reply",
      expansion: "Hello there!"
    });
    expect(coreMocks.invoke).toHaveBeenCalledWith("remove_voice_snippet", { snippetId: "vs-1" });
    expect(coreMocks.invoke).toHaveBeenCalledWith("get_voice_snippet_sync_records", undefined);
    expect(coreMocks.invoke).toHaveBeenCalledWith("reconcile_voice_snippet_records", {
      records: [expect.objectContaining({ normalizedTrigger: "support reply" })]
    });
    expect(coreMocks.invoke.mock.calls.length).toBeGreaterThanOrEqual(61);
  });
});
