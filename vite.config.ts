import { defineConfig } from "vite";

export default defineConfig(async () => ({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  // Разрешаем доступ из Tauri
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    // Tauri использует es2021
    target: "es2021",
    minify: !process.env.TAURI_DEBUG ? "esbuild" : false,
    sourcemap: !!process.env.TAURI_DEBUG,
  },
}));
