import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    port: 5174,
    proxy: {
      "/api/ollama": {
        target: "http://localhost:11434",
        changeOrigin: true,
        rewrite: (p) => p.replace(/^\/api\/ollama/, ""),
      },
      "/api/lmstudio": {
        target: "http://localhost:1234",
        changeOrigin: true,
        rewrite: (p) => p.replace(/^\/api\/lmstudio/, ""),
      },
      "/api/llamacpp": {
        target: "http://localhost:8080",
        changeOrigin: true,
        rewrite: (p) => p.replace(/^\/api\/llamacpp/, ""),
      },
    },
  },
});
