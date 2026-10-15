import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { GpuRuntimeStatus } from "../lib/gpuRuntime";
import { GpuAcceleration } from "./GpuAcceleration";

const gpu = vi.hoisted(() => ({
  getStatus: vi.fn(),
  install: vi.fn(),
  cancel: vi.fn(),
  verify: vi.fn(),
  listen: vi.fn()
}));

vi.mock("../lib/tauri", () => ({ canUseTauri: () => true }));
vi.mock("../lib/gpuRuntime", () => ({
  getGpuRuntimeStatus: gpu.getStatus,
  installGpuRuntime: gpu.install,
  cancelGpuRuntimeInstall: gpu.cancel,
  verifyGpuRuntime: gpu.verify,
  listenGpuRuntimeProgress: gpu.listen
}));

function status(state: GpuRuntimeStatus["state"], overrides: Partial<GpuRuntimeStatus> = {}): GpuRuntimeStatus {
  return {
    state,
    installedVersion: null,
    downloadedBytes: null,
    totalBytes: null,
    percent: null,
    message: "",
    restartRequired: false,
    cudaVerified: false,
    ...overrides
  };
}

describe("GpuAcceleration", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    gpu.listen.mockResolvedValue(vi.fn());
    gpu.getStatus.mockResolvedValue(status("notInstalled"));
  });

  it("shows progress and lets the user cancel an active download", async () => {
    let reportProgress: ((next: GpuRuntimeStatus) => void) | undefined;
    gpu.listen.mockImplementation(async (listener: (next: GpuRuntimeStatus) => void) => {
      reportProgress = listener;
      return vi.fn();
    });
    gpu.cancel.mockResolvedValue(status("cancelled", { message: "Download cancelled." }));

    render(<GpuAcceleration modelId="fw-small.en" />);
    await waitFor(() => expect(screen.getByRole("button", { name: "Download" })).toBeEnabled());
    reportProgress?.(status("downloading", {
      downloadedBytes: 268_435_456,
      totalBytes: 1_073_741_824,
      percent: 25,
      message: "Downloading NVIDIA support…"
    }));

    expect(await screen.findByRole("progressbar", { name: "NVIDIA acceleration download" })).toHaveAttribute("aria-valuenow", "25");
    expect(screen.getByText("256 MB of 1.0 GB · 25%")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

    await waitFor(() => expect(gpu.cancel).toHaveBeenCalledTimes(1));
    expect(await screen.findByRole("button", { name: "Retry" })).toBeEnabled();
  });

  it("recovers from an install error and verifies CUDA after retry", async () => {
    gpu.install
      .mockRejectedValueOnce(new Error("Download interrupted."))
      .mockResolvedValueOnce(status("ready", { installedVersion: "12.4" }));
    gpu.verify.mockResolvedValue(status("ready", {
      installedVersion: "12.4",
      cudaVerified: true,
      message: "NVIDIA acceleration is on and verified."
    }));

    render(<GpuAcceleration modelId="fw-small.en" />);
    fireEvent.click(await screen.findByRole("button", { name: "Download" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Download interrupted.");

    fireEvent.click(screen.getByRole("button", { name: "Download" }));
    await waitFor(() => expect(gpu.verify).toHaveBeenCalledWith("fw-small.en"));
    expect(await screen.findByText("Verified")).toBeInTheDocument();
  });

  it("keeps an installed runtime honest until verification succeeds", async () => {
    gpu.getStatus.mockResolvedValue(status("ready", { installedVersion: "12.4" }));
    gpu.verify.mockResolvedValue(status("failed", { message: "GPU check failed. VoiceWave will keep using the CPU." }));

    render(<GpuAcceleration modelId="fw-small.en" />);
    fireEvent.click(await screen.findByRole("button", { name: "Verify" }));

    expect(await screen.findByText("GPU check failed. VoiceWave will keep using the CPU.")).toBeInTheDocument();
    expect(screen.queryByText("Verified")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Retry" })).toBeEnabled();
  });
});
