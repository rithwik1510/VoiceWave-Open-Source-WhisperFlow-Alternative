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
      // Regression floor, set just under measured coverage (0.7.5: lines 41.7,
      // functions 42.1, statements 41.1, branches 36.6). Re-measure when lowering;
      // this catches big drops without blocking CI on untested UI growth.
      thresholds: {
        lines: 40,
        branches: 35,
        functions: 40,
        statements: 40
      }
    }
  }
});
