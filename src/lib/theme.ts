/* Theme resolution for the main window (plan 014).
 *
 * `data-theme` on <html> is always the RESOLVED theme ("light" | "dark") —
 * never "system" — so all styling can hang off a single attribute selector.
 * The resolved value is mirrored into localStorage for the inline boot script
 * in index.html, which applies it before first paint to kill the light flash. */

export type ThemePreference = "light" | "dark" | "system";

export type ResolvedTheme = "light" | "dark";

export const THEME_STORAGE_KEY = "vw-theme";

const DARK_QUERY = "(prefers-color-scheme: dark)";

function darkMediaQuery(): MediaQueryList | null {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") {
    return null;
  }
  return window.matchMedia(DARK_QUERY);
}

export function resolveTheme(preference: ThemePreference): ResolvedTheme {
  if (preference !== "system") {
    return preference;
  }
  return darkMediaQuery()?.matches ? "dark" : "light";
}

/** The single OS listener; only alive while the preference is "system". */
let systemListener: ((event: MediaQueryListEvent) => void) | null = null;

function unsubscribeFromSystem(): void {
  if (!systemListener) {
    return;
  }
  darkMediaQuery()?.removeEventListener("change", systemListener);
  systemListener = null;
}

/**
 * Applies `preference` to the document and remembers the resolved result.
 * Idempotent: repeated calls converge on exactly one OS listener, and a
 * non-system preference removes it.
 */
export function applyTheme(preference: ThemePreference): ResolvedTheme {
  const resolved = resolveTheme(preference);

  if (typeof document !== "undefined") {
    document.documentElement.dataset.theme = resolved;
  }
  try {
    window.localStorage.setItem(THEME_STORAGE_KEY, resolved);
  } catch {
    // Private-mode / disabled storage: the boot script just re-flashes once.
  }

  if (preference === "system") {
    if (!systemListener) {
      systemListener = () => {
        applyTheme("system");
      };
      darkMediaQuery()?.addEventListener("change", systemListener);
    }
  } else {
    unsubscribeFromSystem();
  }

  return resolved;
}
