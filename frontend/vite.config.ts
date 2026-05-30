import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  build: {
    outDir: "dist",
    emptyOutDir: true,
    rolldownOptions: {
      output: {
        manualChunks(id) {
          if (!id.includes("node_modules")) {
            return undefined;
          }
          if (id.includes("/react/") || id.includes("/react-dom/") || id.includes("/scheduler/")) {
            return "react-vendor";
          }
          if (id.includes("/@tauri-apps/")) {
            return "tauri-vendor";
          }
          if (id.includes("/@ant-design/icons/")) {
            return "antd-icons";
          }
          if (id.includes("/@ant-design/")) {
            return "antd-tools";
          }
          if (id.includes("/rc-")) {
            return "antd-rc";
          }
          if (id.includes("/antd/")) {
            const match = id.match(/\/antd\/es\/([^/]+)/);
            const component = match?.[1];
            if (component === "_util" || component === "config-provider" || component === "locale" || component === "theme" || component === "style") {
              return "antd-core";
            }
            if (component === "table" || component === "list" || component === "descriptions" || component === "tag" || component === "badge" || component === "empty") {
              return "antd-display";
            }
            if (component === "button" || component === "checkbox" || component === "form" || component === "input" || component === "input-number" || component === "select" || component === "switch") {
              return "antd-inputs";
            }
            if (component === "alert" || component === "app" || component === "message" || component === "modal" || component === "notification" || component === "popconfirm" || component === "popover") {
              return "antd-feedback";
            }
            if (component === "layout" || component === "menu" || component === "space" || component === "tabs" || component === "typography") {
              return "antd-shell";
            }
            return "antd-misc";
          }
          return "vendor";
        }
      }
    }
  },
  server: {
    host: "127.0.0.1",
    port: 5173,
    strictPort: true
  }
});
