import { defineConfig, devices } from "@playwright/test";

import type { ServerKind } from "./tests/apps/fixtures";

const browsers = {
  chromium: devices["Desktop Chrome"],
  firefox: devices["Desktop Firefox"],
  webkit: devices["Desktop Safari"],
};
const servers: ServerKind[] = ["js", "js-build", "rust", "spa"];
/** The apps that only some servers run: the app with a Rust server, and the client-only app. */
const only: Partial<Record<ServerKind, string>> = { rust: "blog/**/*.e2e.ts", spa: "spa/**/*.e2e.ts" };

export default defineConfig<object, { serverKind: ServerKind }>({
  testDir: "tests/apps",
  testMatch: "**/*.e2e.ts",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  reporter: process.env.CI ? "github" : "list",
  projects: servers.flatMap((serverKind) =>
    Object.entries(browsers).map(([browser, device]) => ({
      name: `${browser}-${serverKind}`,
      use: { ...device, serverKind },
      ...(only[serverKind] ? { testMatch: only[serverKind] } : { testIgnore: Object.values(only) }),
    })),
  ),
});
