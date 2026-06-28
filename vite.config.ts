import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    host: "127.0.0.1",
    port: 1420,
    strictPort: true,
    proxy: {
      "/api/interpreter": {
        target: "https://overpass-api.de",
        changeOrigin: true,
      },
      "/overpass-api": {
        target: "https://overpass-api.de",
        changeOrigin: true,
        rewrite: (path) => path.replace("/overpass-api", ""),
      },
      "/overpass-kumi": {
        target: "https://overpass.kumi.systems",
        changeOrigin: true,
        rewrite: (path) => path.replace("/overpass-kumi", ""),
      },
      "/overpass-nchc": {
        target: "https://overpass.nchc.org.tw",
        changeOrigin: true,
        rewrite: (path) => path.replace("/overpass-nchc", ""),
      },
    },
    watch: {
      ignored: ["**/src-tauri/target/**", "**/src-tauri/crates/**/target/**"]
    }
  }
});