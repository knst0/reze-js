import { existsSync, readdirSync, rmSync } from "node:fs";
import { join } from "node:path";

import { createBuilder } from "vite";
import { afterAll, describe, expect, test } from "vite-plus/test";

import reze from "../src/index";
import { expectBuildFails } from "./ssg-harness";

const TIMEOUT = 180_000;
const FIXTURE = join(import.meta.dirname, "fixtures", "ssr", "module-state");
const DIST = join(FIXTURE, "dist");

describe("ssr output layout", () => {
  afterAll(() => {
    rmSync(DIST, { recursive: true, force: true });
  });

  test(
    "builds the client into dist/client and the server bundle into dist/server by default",
    async () => {
      const builder = await createBuilder({
        root: FIXTURE,
        configFile: false,
        logLevel: "silent",
        plugins: reze({ ssr: { entry: "src/app.tsx" } }),
      });
      await builder.buildApp();
      expect(existsSync(join(DIST, "client", "reze-assets.json"))).toBe(true);
      expect(existsSync(join(DIST, "server", "entry.js"))).toBe(true);
      expect(readdirSync(DIST).sort()).toEqual(["client", "server"]);
    },
    TIMEOUT,
  );

  test(
    "refuses a server directory inside the client directory",
    async () => {
      await expectBuildFails(reze({ ssr: { entry: "src/app.tsx" } }), FIXTURE, DIST, /overlaps build\.outDir/);
    },
    TIMEOUT,
  );

  test(
    "refuses a server directory that contains the client directory",
    async () => {
      await expectBuildFails(
        reze({ ssr: { entry: "src/app.tsx", outDir: "dist" } }),
        FIXTURE,
        join(DIST, "client"),
        /overlaps build\.outDir/,
      );
    },
    TIMEOUT,
  );
});
