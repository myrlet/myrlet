import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

// The Hub serves the built UI same-origin in production; during development the
// Vite server proxies the Admin API (including the SSE stream) to a loopback Hub.
const HUB_TARGET = "http://127.0.0.1:4140";

export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    proxy: {
      "/v1": { target: HUB_TARGET, changeOrigin: true },
      "/health": { target: HUB_TARGET, changeOrigin: true },
    },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
  },
  test: {
    environment: "jsdom",
    globals: true,
  },
});
