import { join } from "node:path";

import { playwright } from "@vitest/browser-playwright";
import type { PluginOption } from "vite";
import { configDefaults, defineConfig, type TestProjectInlineConfiguration } from "vitest/config";

type BrowserConfig = NonNullable<TestProjectInlineConfiguration["test"]>["browser"];

const supportedBrowsers = ["chromium", "firefox", "webkit"] as const;
type SupportedBrowser = (typeof supportedBrowsers)[number];

const packages = join(import.meta.dirname, "packages");

/** Resolves every workspace package to its sources, so tests never read a stale `dist`. */
export const sourceAliases = [
  { find: "@rezejs/router/fs", replacement: join(packages, "router", "src", "fs", "index.ts") },
  { find: "@rezejs/router", replacement: join(packages, "router", "src", "index.ts") },
  { find: "reze-js", replacement: join(packages, "reze-js", "src", "index.ts") },
  { find: "@rezejs/dom/jsx-runtime", replacement: join(packages, "dom", "src", "jsx-runtime.ts") },
  { find: "@rezejs/dom", replacement: join(packages, "dom", "src", "index.ts") },
  { find: "@rezejs/signals/render", replacement: join(packages, "signals", "src", "render.ts") },
  { find: "@rezejs/signals/devtools", replacement: join(packages, "signals", "src", "devtools.ts") },
  { find: "@rezejs/signals", replacement: join(packages, "signals", "src", "index.ts") },
];

function isSupportedBrowser(name: string): name is SupportedBrowser {
  return (supportedBrowsers as readonly string[]).includes(name);
}

/**
 * Browser mode for `VITEST_ENV`: `chromium`, `firefox`, `webkit` or `all-browsers`. Unset or empty
 * returns `undefined`, leaving the project on its non-browser environment; any other value throws.
 */
export function browserConfig(environment = process.env.VITEST_ENV): BrowserConfig {
  if (!environment) return undefined;
  let instances: { browser: SupportedBrowser }[];
  if (environment === "all-browsers") {
    instances = supportedBrowsers.map((browser) => ({ browser }));
  } else if (isSupportedBrowser(environment)) {
    instances = [{ browser: environment }];
  } else {
    throw new Error(`VITEST_ENV: unknown "${environment}", expected "all-browsers" or one of ${supportedBrowsers.join(", ")}`);
  }
  return {
    enabled: true,
    provider: playwright({ contextOptions: { timezoneId: "UTC" } }),
    screenshotFailures: false,
    headless: true,
    instances,
  };
}

export const nodeSpecs = "tests/**/*.node.spec.ts";

export function domProject(name: string, plugins: PluginOption[]) {
  return defineConfig({
    plugins,
    test: {
      name,
      exclude: [...configDefaults.exclude, nodeSpecs],
      environment: "happy-dom",
      environmentOptions: { happyDOM: { url: "http://localhost/" } },
      alias: sourceAliases,
      browser: browserConfig(),
    },
  });
}

export function nodeProject(name: string) {
  return defineConfig({ test: { name: `${name} (node)`, include: [nodeSpecs] } });
}
