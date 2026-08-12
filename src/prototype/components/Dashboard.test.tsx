import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { THEME } from "../constants";
import { Dashboard } from "./Dashboard";

describe("Dashboard", () => {
  it("renders normalized model label for fw-small.en", () => {
    render(
      <Dashboard
        theme={THEME}
        status="idle"
        onPressStart={vi.fn()}
        onPressEnd={vi.fn()}
        currentModel="fw-small.en"
        partialTranscript={null}
        finalTranscript={null}
        pushToTalkHotkey="Ctrl+Windows"
      />
    );

    expect(screen.getByText("FW SMALL.EN")).toBeInTheDocument();
  });

  it("personalizes the greeting when a signed-in name is available", () => {
    render(
      <Dashboard
        theme={THEME}
        status="idle"
        onPressStart={vi.fn()}
        onPressEnd={vi.fn()}
        currentModel="fw-small.en"
        partialTranscript={null}
        finalTranscript={null}
        pushToTalkHotkey="Ctrl+Windows"
        userName="Rishi"
      />
    );

    expect(screen.getByText("Welcome back, Rishi.")).toBeInTheDocument();
  });

  it("keeps hold-to-talk lifecycle on pointer press and release", () => {
    const onPressStart = vi.fn();
    const onPressEnd = vi.fn();

    render(
      <Dashboard
        theme={THEME}
        status="idle"
        onPressStart={onPressStart}
        onPressEnd={onPressEnd}
        currentModel="fw-large-v3"
        partialTranscript={null}
        finalTranscript={null}
        pushToTalkHotkey="Ctrl+Windows"
      />
    );

    const holdButton = screen.getByRole("button", { name: "Hold to dictate" });
    fireEvent.pointerDown(holdButton, { pointerId: 1 });
    fireEvent.pointerUp(holdButton, { pointerId: 1 });
    fireEvent.pointerCancel(holdButton);

    expect(onPressStart).toHaveBeenCalledTimes(1);
    expect(onPressEnd).toHaveBeenCalledTimes(1);
  });

  it("explains the shared double-tap shortcut and hands-free finish gesture", () => {
    const { rerender } = render(
      <Dashboard
        theme={THEME}
        status="idle"
        onPressStart={vi.fn()}
        onPressEnd={vi.fn()}
        currentModel="fw-small.en"
        partialTranscript={null}
        finalTranscript={null}
        pushToTalkHotkey="Ctrl+Windows"
      />
    );

    expect(screen.getByText("Double-tap for hands-free")).toBeInTheDocument();
    expect(screen.queryByText("Ctrl+Alt+X")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /hands-free/i })).not.toBeInTheDocument();

    rerender(
      <Dashboard
        theme={THEME}
        status="listening"
        controlMode="handsFree"
        onPressStart={vi.fn()}
        onPressEnd={vi.fn()}
        currentModel="fw-small.en"
        partialTranscript={null}
        finalTranscript={null}
        pushToTalkHotkey="Ctrl+Windows"
      />
    );

    expect(screen.getByText("Hands-free listening...")).toBeInTheDocument();
    expect(screen.getByText("Pause when done, or press Ctrl+Windows again.")).toBeInTheDocument();
  });
});
