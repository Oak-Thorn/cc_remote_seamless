import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import svgLoader from "vite-svg-loader";

export default defineConfig({
  plugins: [
    vue(),
    svgLoader({ defaultImport: "component" }),
  ],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ["**/third_service/**", "**/target/**", "**/dist/**"],
    },
  },
  optimizeDeps: {
    // Vite's default entry scan globs **/*.html. Pin to the real app entry so
    // it doesn't sweep into third_service/ and overwhelm esbuild (EPIPE).
    entries: ["index.html"],
  },
  envPrefix: ["VITE_", "TAURI_"],
});
