import { defineConfig, devices } from "@playwright/test";

import type { ServerKind } from "./tests/apps/fixtures";

const browsers = {
  chromium: devices["Desktop Chrome"],
  firefox: devices["Desktop Firefox"],
  webkit: devices["Desktop Safari"],
};
const servers: ServerKind[] = ["js", "js-build", "rust"];
/** The apps with a Rust server crate, which only the `rust` server runs. */
const rust = "blog/**";

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
      ...(serverKind === "rust" ? { testMatch: `${rust}/*.e2e.ts` } : { testIgnore: rust }),
    })),
  ),
});
