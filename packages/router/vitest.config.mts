import { join } from "node:path";

import { defineConfig } from "vitest/config";

import reze from "../vite-plugin/src/index";

const packages = join(import.meta.dirname, "..");

export default defineConfig({
  plugins: [reze({ links: "@rezejs/router" })],
  test: {
    environment: "happy-dom",
    environmentOptions: { happyDOM: { url: "http://localhost/" } },
    alias: [
      { find: "@rezejs/router/vite", replacement: join(packages, "router", "src", "vite", "index.ts") },
      { find: "@rezejs/router", replacement: join(packages, "router", "src", "index.ts") },
      { find: "reze-js", replacement: join(packages, "reze-js", "src", "index.ts") },
      { find: "@rezejs/dom/jsx-runtime", replacement: join(packages, "dom", "src", "jsx-runtime.ts") },
      { find: "@rezejs/dom", replacement: join(packages, "dom", "src", "index.ts") },
      { find: "@rezejs/signals/render", replacement: join(packages, "signals", "src", "render.ts") },
      { find: "@rezejs/signals/devtools", replacement: join(packages, "signals", "src", "devtools.ts") },
      { find: "@rezejs/signals", replacement: join(packages, "signals", "src", "index.ts") },
    ],
  },
});
