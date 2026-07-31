import { afterEach, describe, expect, it, vi } from "vitest";
import { resetAnonymousUsageForTests, trackAnonymousUsage } from "./telemetry";

class MemoryStorage {
  private values = new Map<string, string>();

  getItem(key: string): string | null {
    return this.values.get(key) ?? null;
  }

  setItem(key: string, value: string): void {
    this.values.set(key, value);
  }
}

afterEach(() => {
  resetAnonymousUsageForTests();
});

describe("anonymous activated-install analytics", () => {
  it("does nothing before explicit consent", async () => {
    const signal = vi.fn();
    const storage = new MemoryStorage();

    const result = await trackAnonymousUsage(false, {
      appId: "test-app",
      storage,
      randomUUID: () => "install-1",
      getAppVersion: async () => "0.5.8",
      createClient: () => ({ signal })
    });

    expect(result).toEqual({
      configured: true,
      activationSent: false,
      dailyActiveSent: false
    });
    expect(signal).not.toHaveBeenCalled();
    expect(storage.getItem("voicewave.analytics.installation-id")).toBeNull();
  });

  it("sends activation once and active usage once per UTC day", async () => {
    const signal = vi.fn().mockResolvedValue({ ok: true });
    const storage = new MemoryStorage();
    const dependencies = {
      appId: "test-app",
      storage,
      now: () => Date.UTC(2026, 6, 31, 12),
      randomUUID: () => "install-1",
      getAppVersion: async () => "0.5.8",
      createClient: () => ({ signal: signal as (type: string, payload?: Record<string, string>) => Promise<Response> })
    };

    const first = await trackAnonymousUsage(true, dependencies);
    const second = await trackAnonymousUsage(true, dependencies);

    expect(first).toEqual({
      configured: true,
      activationSent: true,
      dailyActiveSent: true
    });
    expect(second).toEqual({
      configured: true,
      activationSent: false,
      dailyActiveSent: false
    });
    expect(signal).toHaveBeenCalledTimes(2);
    expect(signal).toHaveBeenNthCalledWith(1, "App.installActivated", {
      "VoiceWave.App.version": "0.5.8",
      "VoiceWave.Distribution.channel": "github"
    });
    expect(signal).toHaveBeenNthCalledWith(2, "App.activeDaily", {
      "VoiceWave.App.version": "0.5.8",
      "VoiceWave.Distribution.channel": "github"
    });
  });

  it("does not mark failed events as sent", async () => {
    const signal = vi.fn().mockResolvedValue({ ok: false });
    const storage = new MemoryStorage();
    const dependencies = {
      appId: "test-app",
      storage,
      now: () => Date.UTC(2026, 6, 31, 12),
      randomUUID: () => "install-1",
      getAppVersion: async () => "0.5.8",
      createClient: () => ({ signal: signal as (type: string, payload?: Record<string, string>) => Promise<Response> })
    };

    await trackAnonymousUsage(true, dependencies);
    await trackAnonymousUsage(true, dependencies);

    expect(signal).toHaveBeenCalledTimes(4);
  });
});
