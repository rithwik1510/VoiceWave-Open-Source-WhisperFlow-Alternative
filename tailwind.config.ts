import type { Config } from "tailwindcss";

const config: Config = {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        /* Semantic theme tokens (plan 014). Every neutral in the main-window
           TSX resolves through these so light/dark is one CSS variable swap. */
        ink: "var(--vw-ink)",
        "ink-strong": "var(--vw-color-text-primary)",
        "ink-hover": "var(--vw-ink-hover)",
        "ink-contrast": "var(--vw-ink-contrast)",
        sub: "var(--vw-color-text-secondary)",
        faint: "var(--vw-color-text-muted)",
        label: "var(--vw-color-text-label)",
        quiet: "var(--vw-color-text-quiet)",
        hint: "var(--vw-color-text-hint)",
        surface: "var(--vw-color-surface)",
        page: "var(--vw-color-surface-soft)",
        inset: "var(--vw-color-surface-subtle)",
        edge: "var(--vw-color-border)",
        "edge-strong": "var(--vw-color-border-strong)",
        hairline: "var(--vw-color-divider)",
        track: "var(--vw-track-quiet)",
        accent: "var(--vw-accent-blue-600)",
        "accent-deep": "var(--vw-accent-navy-900)",
        "on-accent": "var(--vw-on-accent)",
        "accent-text": "var(--vw-accent-text)",
        "accent-rule": "var(--vw-accent-rule)",
        "state-error": "var(--vw-state-error-accent)",
        "status-success": "var(--vw-status-success)",
        "status-success-text": "var(--vw-status-success-text)",
        "status-warn-text": "var(--vw-status-warn-text)",
        "status-warn-text-soft": "var(--vw-status-warn-text-soft)",
        "status-warn-bg": "var(--vw-status-warn-bg)",
        "status-warn-border": "var(--vw-status-warn-border)",
        "status-danger-text": "var(--vw-status-danger-text)",
        "status-danger-bg": "var(--vw-status-danger-bg)",
        "status-danger-border": "var(--vw-status-danger-border)",
        "status-info-bg": "var(--vw-status-info-bg)",
        "status-info-border": "var(--vw-status-info-border)",
        pine: {
          50: "#f3f8ff",
          100: "#e1ecff",
          200: "#c8dcff",
          300: "#a6c5ff",
          500: "#3568b3",
          700: "#244b83",
          900: "#132b57"
        }
      },
      boxShadow: {
        card: "0 10px 32px -18px rgba(36, 75, 131, 0.45)"
      },
      fontFamily: {
        display: ["\"Fraunces\"", "Georgia", "serif"],
        body: ["\"DM Sans\"", "Segoe UI", "sans-serif"]
      }
    }
  },
  plugins: []
};

export default config;
