import type { Plugin, UserConfig } from "vite";
import { expect, test } from "vitest";

import reze, { type Options } from "../src/index";

function configFor(options: Options, userConfig: UserConfig): unknown {
  const plugin = reze(options) as Plugin;
  const hook = plugin.config as (config: UserConfig) => unknown;
  return hook(userConfig);
}

test("turns the modulepreload polyfill off by default", () => {
  expect(configFor({}, {})).toEqual({ build: { modulePreload: { polyfill: false } } });
});

test("keeps the polyfill when asked to", () => {
  expect(configFor({ modulePreloadPolyfill: true }, {})).toBeUndefined();
});

test("leaves an explicit build.modulePreload alone", () => {
  expect(configFor({}, { build: { modulePreload: false } })).toBeUndefined();
  expect(configFor({}, { build: { modulePreload: { polyfill: true } } })).toBeUndefined();
});
