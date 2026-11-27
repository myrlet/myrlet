import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

// `vite.config.ts` runs in Node. Declaring the tiny surface we use keeps the
// config strict without pulling Node typings into the browser application.
declare const process: { env: Record<string, string | undefined> };

// The Hub serves the built UI same-origin in production; during development the
// Vite server proxies the Admin API (including the SSE stream) to a Hub.
//
// Homelab development can point straight at a deployed Hub with zero CORS:
//   VITE_HUB_PROXY=https://myrlet.fabianbao.xyz npm run dev
const HUB_TARGET = process.env.VITE_HUB_PROXY || "http://127.0.0.1:4140";

export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    proxy: {
      "/v1": { target: HUB_TARGET, changeOrigin: true, secure: true },
      "/health": { target: HUB_TARGET, changeOrigin: true, secure: true },
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
