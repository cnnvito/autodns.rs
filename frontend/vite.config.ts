import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Desktop bundle is loaded from local disk by the Tauri webview, so fine-grained
// vendor chunking buys nothing; page-level lazy imports already split the app.
export default defineConfig({
  plugins: [react()],
  build: {
    outDir: "dist",
    emptyOutDir: true,
    chunkSizeWarningLimit: 4096
  },
  server: {
    host: "127.0.0.1",
    port: 5173,
    strictPort: true
  }
});
