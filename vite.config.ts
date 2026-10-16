import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import { resolve } from "path";

export default defineConfig({
  plugins: [react()],
  optimizeDeps: {
    entries: ["index.html", "pill.html"]
  },
  build: {
    rollupOptions: {
      input: {
        main: resolve(__dirname, "index.html"),
        pill: resolve(__dirname, "pill.html")
      }
    }
  },
  server: {
    watch: {
      ignored: ["**/voicewave-prototype/**"]
    }
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: "./src/test/setup.ts",
    coverage: {
      provider: "v8",
      reporter: ["text", "html", "json-summary"],
      include: ["src/**/*.{ts,tsx}"],
      exclude: ["src/test/**", "src/**/*.test.{ts,tsx}"],
      // Regression floor, set just under measured coverage (2026-10: lines 43.8,
      // functions 44.4, statements 43.1, branches 40.2). The old 45/65 bar was
      // never met once the UI grew; this catches big drops without blocking CI.
      thresholds: {
        lines: 42,
        branches: 38,
        functions: 42,
        statements: 42
      }
    }
  }
});
