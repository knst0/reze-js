import { EventEmitter } from "node:events";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { computed, signal } from "@rezejs/signals";
import { profileComponent, startProfileSession, stopProfileSession } from "@rezejs/signals/profile";
import type { ResolvedConfig } from "vite";
import { afterEach, beforeEach, expect, test, vi, type Mock } from "vitest";

import reze from "../src";

const source = [
  'import { signal } from "@rezejs/signals";',
  "const [a, setA] = signal(0);",
  "const [b, setB] = signal(0);",
  "export const view = <p title={a()} data-x={b()} onClick={() => { setA(1); setB(2); }}>hi</p>;",
].join("\n");

const file = "/src/View.tsx";

const serveConfig = {
  command: "serve",
  isProduction: false,
  server: { hmr: false },
} as unknown as ResolvedConfig;

const environment = {
  consumer: "client",
  dev: { sourcemap: false },
  build: { sourcemap: false },
};

let dir: string;

beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), "reze-loop-"));
});

afterEach(() => {
  rmSync(dir, { recursive: true, force: true });
});

function effects(code: string): number {
  return (code.match(/renderEffect/g) ?? []).length;
}
function loop(profileDir: string) {
  const plugin = reze({ profile: { dir: profileDir } });
  (plugin.configResolved as (config: ResolvedConfig) => void)({ ...serveConfig, root: dir });
  const use = vi.fn();
  (plugin.configureServer as unknown as (server: { middlewares: { use: Mock } }) => void)({
    middlewares: { use },
  });
  const handler = use.mock.calls[0]![1] as (req: unknown, res: unknown, next: () => void) => void;
  const hook = plugin.transform as { handler: (code: string, id: string) => unknown };
  const context = { environment: { config: environment }, warn: vi.fn() };
  return {
    transform: () => hook.handler.call(context, source, file) as { code: string; map: null },
    post: (body: unknown) => {
      const req = Object.assign(new EventEmitter(), { method: "POST" });
      const res = { statusCode: 0, end: vi.fn() };
      handler(req, res, vi.fn());
      req.emit("data", JSON.stringify(body));
      req.emit("end");
      return res;
    },
  };
}

test("a posted session specializes the next transform of the same file", () => {
  const { transform, post } = loop(join(dir, "profiles"));
  expect(effects(transform().code)).toBe(4);

  startProfileSession();
  profileComponent("view", `${file}#view`, 0, () => {
    const [a, setA] = signal(0);
    const [b, setB] = signal(0);
    const title = computed(() => a());
    const data = computed(() => b());
    title();
    data();
    setA(1);
    setB(2);
  });
  const tree = stopProfileSession();
  expect(tree.components).toEqual([{ component: "view", file, mounts: 1, props: 0, reruns: 0, writes: 2 }]);

  expect(post(tree).statusCode).toBe(200);
  expect(effects(transform().code)).toBe(3);
});
