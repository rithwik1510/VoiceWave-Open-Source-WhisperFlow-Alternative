import type React from "react";

export interface ThemeConfig {
  id: string;
  name: string;
  description: string;
  logo: React.ElementType;
  colors: {
    bg: string;
    sidebarBg: string;
    surface: string;
    surfaceHighlight: string;
    border: string;
    textPrimary: string;
    textSecondary: string;
    textTertiary: string;
    accent: string;
    accentFg: string;
    success: string;
    warning: string;
    error: string;
    recording: string;
    navActiveBg: string;
    navActiveFg: string;
    shellBg: string;
    canvasBg: string;
    canvasBorder: string;
    surfaceBorder: string;
    divider: string;
    accentBlue: string;
    accentCyan: string;
    accentGradient: string;
    accentGradientSoft: string;
  };
  typography: {
    fontDisplay: string;
    fontBody: string;
    weightHeading: string;
  };
  shapes: {
    radius: string;
    borderWidth: string;
    buttonShape: string;
    navItemShape: string;
  };
  effects: {
    shadow: string;
    blur: string;
  };
}

export type DictationState = "idle" | "listening" | "transcribing" | "inserted" | "error";

/** Full pages in the main canvas. */
export type PageId = "home" | "models" | "dictionary" | "snippets" | "sessions" | "stats";
/** Panels that open as overlays above the current page. "profile" and "auth"
 * only exist in builds with cloud sync configured. */
export type OverlayId = "style" | "settings" | "help" | "profile" | "auth";
export type NavId = PageId | OverlayId;

export interface SessionData {
  id: string;
  title: string;
  preview: string;
  date: string;
  duration: string;
  wordCount: number;
}
