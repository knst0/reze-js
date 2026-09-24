import { fileRoutes } from "@rezejs/router/vite";
import reze from "@rezejs/vite-plugin";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [reze(), fileRoutes()],
});
