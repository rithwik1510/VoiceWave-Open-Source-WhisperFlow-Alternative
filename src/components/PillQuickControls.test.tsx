import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { PillQuickControls } from "./PillQuickControls";

function renderControls(overrides: Partial<Parameters<typeof PillQuickControls>[0]> = {}) {
  const props: Parameters<typeof PillQuickControls>[0] = {
    invocation: "mouse",
    activeProfile: "standard",
    pendingProfile: null,
    startingHandsFree: false,
    feedback: null,
    onSelectProfile: vi.fn(),
    onStartHandsFree: vi.fn(),
    onOpenMain: vi.fn(),
    onClose: vi.fn(),
    onActivity: vi.fn(),
    ...overrides
  };
  render(<PillQuickControls {...props} />);
  return props;
}

describe("PillQuickControls", () => {
  it("exposes a dedicated close action without coupling it to Open VoiceWave", () => {
    const props = renderControls();

    fireEvent.click(screen.getByRole("button", { name: "Close Quick Controls" }));

    expect(props.onClose).toHaveBeenCalledOnce();
    expect(props.onOpenMain).not.toHaveBeenCalled();
  });

  it("selects the exact typed profile and marks the active profile", () => {
    const props = renderControls({ activeProfile: "writing" });

    expect(screen.getByRole("radio", { name: "Writing" })).toHaveAttribute(
      "aria-checked",
      "true"
    );
    fireEvent.click(screen.getByRole("radio", { name: "Coding" }));
    expect(props.onSelectProfile).toHaveBeenCalledWith("coding");
  });

  it("moves and selects profiles with arrow keys", () => {
    const props = renderControls({ activeProfile: "standard" });
    const standard = screen.getByRole("radio", { name: "Standard" });
    standard.focus();

    fireEvent.keyDown(standard, { key: "ArrowRight" });

    expect(screen.getByRole("radio", { name: "Writing" })).toHaveFocus();
    expect(props.onSelectProfile).toHaveBeenCalledWith("writing");
  });

  it("keeps the profile label stable while the selection is saving", () => {
    renderControls({ pendingProfile: "writing" });

    const writing = screen.getByRole("radio", { name: "Writing" });
    expect(writing).toHaveTextContent("Writing");
    expect(writing).toHaveAttribute("aria-busy", "true");
    expect(screen.queryByText("Saving…")).not.toBeInTheDocument();
  });

  it("starts hands-free and focuses Close only for the focused entry path", () => {
    const props = renderControls({ invocation: "focused" });
    expect(screen.getByRole("button", { name: "Close Quick Controls" })).toHaveFocus();

    fireEvent.click(screen.getByRole("button", { name: "Start hands-free" }));
    expect(props.onStartHandsFree).toHaveBeenCalledOnce();
  });
});
