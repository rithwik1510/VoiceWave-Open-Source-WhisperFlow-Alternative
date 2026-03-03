import type { Config } from "tailwindcss";

const config: Config = {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
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
