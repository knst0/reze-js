import { configDefaults, defineConfig } from "vite-plus";

import { browserConfig, nodeSpecs } from "../../vitest.shared";

export default defineConfig({
  define: {
    __REZE_HTML__: "false",
  },
  test: {
    name: "@rezejs/signals",
    exclude: [...configDefaults.exclude, nodeSpecs],
    browser: browserConfig(),
  },
});
