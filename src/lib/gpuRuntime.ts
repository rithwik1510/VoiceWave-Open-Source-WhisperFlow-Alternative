import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type GpuRuntimeState =
  | "notInstalled"
  | "downloading"
  | "installing"
  | "verifying"
  | "ready"
  | "failed"
  | "cancelled";

export interface GpuRuntimeStatus {
  state: GpuRuntimeState;
  installedVersion: string | null;
  downloadedBytes: number | null;
  totalBytes: number | null;
  percent: number | null;
  message: string;
  restartRequired: boolean;
  cudaVerified: boolean;
}

export function getGpuRuntimeStatus(): Promise<GpuRuntimeStatus> {
  return invoke<GpuRuntimeStatus>("gpu_runtime_status");
}

export function installGpuRuntime(): Promise<GpuRuntimeStatus> {
  return invoke<GpuRuntimeStatus>("install_gpu_runtime");
}

export function cancelGpuRuntimeInstall(): Promise<GpuRuntimeStatus> {
  return invoke<GpuRuntimeStatus>("cancel_gpu_runtime_install");
}

export function verifyGpuRuntime(modelId: string): Promise<GpuRuntimeStatus> {
  return invoke<GpuRuntimeStatus>("verify_gpu_runtime", { modelId });
}

export function listenGpuRuntimeProgress(
  onStatus: (status: GpuRuntimeStatus) => void
): Promise<UnlistenFn> {
  return listen<GpuRuntimeStatus>("voicewave://gpu-runtime-progress", (event) => onStatus(event.payload));
}
