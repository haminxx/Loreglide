import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri expects a fixed dev port and specific output dir.
// See https://tauri.app/v1/guides/getting-started/setup/vite
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: "127.0.0.1",
    hmr: { port: 1421 },
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target: "es2021",
    minify: "esbuild",
    sourcemap: !!process.env.TAURI_DEBUG,
  },
});
