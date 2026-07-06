import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { Onboarding } from "./Onboarding";
import type { ModelCatalogItem, VoiceWaveSnapshot } from "../types/voicewave";

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
