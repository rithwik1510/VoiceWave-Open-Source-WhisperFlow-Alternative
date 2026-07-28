import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { Onboarding } from "./Onboarding";
import type { ModelCatalogItem, ModelStatus, VoiceWaveSnapshot } from "../types/voicewave";

const CATALOG: ModelCatalogItem[] = [
  {
    modelId: "fw-small.en",
    displayName: "Faster-Whisper Small English",
    version: "1",
    format: "ct2",
    sizeBytes: 466 * 1024 * 1024,
    sha256: "x",
    license: "MIT",
    downloadUrl: "https://example.invalid"
  } as ModelCatalogItem
];

function makeSnapshot(overrides: Partial<VoiceWaveSnapshot> = {}): VoiceWaveSnapshot {
  return {
    state: "idle",
    lastPartial: null,
    lastFinal: null,
    activeModel: "fw-small.en",
    ...overrides
  };
}

function renderFlow(overrides: Partial<Parameters<typeof Onboarding>[0]> = {}) {
  const props = {
    catalog: CATALOG,
    statuses: {},
    hasInstalledModel: true,
    installModel: vi.fn().mockResolvedValue(undefined),
    makeModelActive: vi.fn().mockResolvedValue(undefined),
    hotkeyLabel: "Ctrl+Windows",
    snapshot: makeSnapshot(),
    onComplete: vi.fn(),
    ...overrides
  };
  const view = render(<Onboarding {...props} />);
  return { props, view };
}

describe("Onboarding first-run flow", () => {
  it("renders the welcome step and skips on demand", () => {
    const { props } = renderFlow();

    expect(screen.getByText(/Your voice,/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Skip setup" }));
    expect(props.onComplete).toHaveBeenCalledTimes(1);
  });

  it("starts the model download when none is installed", () => {
    const { props } = renderFlow({ hasInstalledModel: false });

    fireEvent.click(screen.getByRole("button", { name: /Set up VoiceWave/ }));
    expect(props.installModel).toHaveBeenCalledWith("fw-small.en");
    expect(screen.getByText("Say something.")).toBeInTheDocument();
  });

  it("offers Retry when the download fails instead of freezing on the progress card", async () => {
    // What the user hits with no internet: the install call rejects and the
    // backend reports the failure as a `failed` status (state.rs) which the
    // hook mirrors into modelStatuses (useVoiceWave installModel catch).
    const installModel = vi.fn().mockRejectedValue(new Error("prefetch failed"));
    const { props, view } = renderFlow({ hasInstalledModel: false, installModel });

    fireEvent.click(screen.getByRole("button", { name: /Set up VoiceWave/ }));
    expect(installModel).toHaveBeenCalledTimes(1);
    // Flush the rejected install promise before asserting on the retry state.
    await act(async () => {});

    fireEvent.click(screen.getByRole("button", { name: /Continue/ }));

    const failed: ModelStatus = {
      modelId: "fw-small.en",
      state: "failed",
      progress: 0,
      active: false,
      installed: false,
      message: "Couldn't download the speech model. Check your internet connection and press Retry.",
      installedModel: null,
      downloadedBytes: 0,
      totalBytes: 466 * 1024 * 1024,
      resumable: false
    };
    view.rerender(<Onboarding {...props} statuses={{ "fw-small.en": failed }} />);

    expect(screen.getByText("The model download hit a snag.")).toBeInTheDocument();
    expect(screen.getByText(/Check your internet connection/)).toBeInTheDocument();
    // Not stuck: setup can be finished (or retried) instead of dead-ending.
    expect(screen.getByRole("button", { name: /Continue/ })).not.toBeDisabled();

    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(installModel).toHaveBeenCalledTimes(2);
    expect(installModel).toHaveBeenLastCalledWith("fw-small.en");
    await act(async () => {});
  });

  it("walks mic check into the rehearsal step and celebrates the first dictation", () => {
    const { props, view } = renderFlow();

    fireEvent.click(screen.getByRole("button", { name: /Get started/ }));
    expect(screen.getByText("Say something.")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /Continue/ }));
    expect(screen.getByText("Hold the key. Speak. Let go.")).toBeInTheDocument();
    expect(screen.getByText("Ctrl+Windows")).toBeInTheDocument();

    // A real dictation lands as a new final transcript on the snapshot.
    view.rerender(
      <Onboarding {...props} snapshot={makeSnapshot({ lastFinal: "Made coffee and read the news." })} />
    );
    expect(screen.getByText(/That's the whole trick/)).toBeInTheDocument();
    expect(screen.getByLabelText("Dictation playground")).toHaveValue(
      "Made coffee and read the news."
    );

    fireEvent.click(screen.getByRole("button", { name: /Continue/ }));
    expect(screen.getByText("You're set.")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Start dictating" }));
    expect(props.onComplete).toHaveBeenCalledTimes(1);
  });
});
