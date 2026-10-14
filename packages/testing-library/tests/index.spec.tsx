import { signal } from "@rezejs/signals";
import { afterEach, expect, test } from "vite-plus/test";

import { cleanup, deferred, fire, mount, tick } from "../src/index";

afterEach(cleanup);

test("signal update re-renders mounted output", () => {
  const [name, setName] = signal("a");
  const { el } = mount(() => <p>hi {name()}</p>);
  expect(el.textContent).toContain("hi a");
  setName("b");
  tick();
  expect(el.textContent).toContain("hi b");
});

test("cleanup empties document.body and dispose is idempotent", () => {
  const mounted = mount(() => <p>hi</p>);
  expect(document.body.contains(mounted.el)).toBe(true);
  mounted.dispose();
  mounted.dispose();
  cleanup();
  expect(document.body.textContent).toBe("");
  cleanup();
  expect(document.body.textContent).toBe("");
});

test("fire dispatches a click that runs the handler", () => {
  let clicks = 0;
  const { el } = mount(() => <button onClick={() => clicks++}>go</button>);
  const button = el.querySelector("button")!;
  expect(fire(button, "click")).toBe(true);
  expect(clicks).toBe(1);
});

test("deferred counts load() calls and allows retry after reject", async () => {
  const source = deferred<string>();
  const first = source.load();
  expect(source.calls()).toBe(1);
  source.reject(new Error("nope"));
  await expect(first).rejects.toThrow("nope");
  const second = source.load();
  expect(source.calls()).toBe(2);
  source.resolve("ok");
  await expect(second).resolves.toBe("ok");
});
