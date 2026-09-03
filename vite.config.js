import { defineConfig } from "vite";

// Tauri expects a fixed port and a dist output.
export default defineConfig({
  root: ".",
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    target: "safari15",
  },
});
