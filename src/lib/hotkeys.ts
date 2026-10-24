/** Hold-to-talk default; mirrors DEFAULT_PUSH_TO_TALK_HOTKEY in settings/mod.rs. */
export const DEFAULT_PUSH_TO_TALK_HOTKEY = "Ctrl+Windows";
/** Fixed hands-free toggle; mirrors LOCKED_TOGGLE_HOTKEY in settings/mod.rs. */
export const LOCKED_TOGGLE_HOTKEY = "Ctrl+Alt+X";

const KEY_LABELS: Record<string, string> = {
  CTRL: "Ctrl",
  CONTROL: "Ctrl",
  WINDOWS: "Win",
  WIN: "Win",
  SUPER: "Win",
  META: "Win",
  PAGEUP: "Page Up",
  PAGEDOWN: "Page Down",
  SCROLLLOCK: "Scroll Lock"
};

/** Keycap labels for a stored shortcut: "Ctrl+Windows" -> ["Ctrl", "Win"]. */
export function hotkeyKeys(combo: string): string[] {
  return combo
    .split("+")
    .map((part) => part.trim())
    .filter(Boolean)
    .map((part) => KEY_LABELS[part.toUpperCase()] ?? part);
}

/** One-line label for copy: "Ctrl+Windows" -> "Ctrl + Win". */
export function formatHotkey(combo: string): string {
  return hotkeyKeys(combo).join(" + ");
}
