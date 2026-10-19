import { useCallback, useEffect, useState } from "react";

import { canUseTauri } from "../lib/tauri";

/**
 * "Start with Windows". The OS Run-key entry is the source of truth (the user
 * can also switch it off in Task Manager > Startup apps), so this reads it
 * instead of mirroring a saved setting. `null` while loading or outside the
 * desktop app, where the row is not shown.
 */
export function useAutostart() {
  const [enabled, setEnabledState] = useState<boolean | null>(null);

  useEffect(() => {
    if (!canUseTauri()) {
      return;
    }
    let cancelled = false;
    void import("@tauri-apps/plugin-autostart")
      .then(({ isEnabled }) => isEnabled())
      .then((value) => {
        if (!cancelled) {
          setEnabledState(value);
        }
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, []);

  const setEnabled = useCallback(async (next: boolean) => {
    setEnabledState(next);
    try {
      const { enable, disable, isEnabled } = await import("@tauri-apps/plugin-autostart");
      await (next ? enable() : disable());
      setEnabledState(await isEnabled());
    } catch {
      // Registry write refused: show what Windows actually has.
      const { isEnabled } = await import("@tauri-apps/plugin-autostart");
      setEnabledState(await isEnabled().catch(() => !next));
    }
  }, []);

  return { enabled, setEnabled };
}
