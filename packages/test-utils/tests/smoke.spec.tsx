import { signal } from "@rezejs/signals";
import { afterEach, expect, test } from "vitest";

import { cleanup, fire, mount, tick } from "../src";

afterEach(cleanup);

test("mount renders, fire clicks, tick flushes", () => {
  const [count, setCount] = signal(0);
  const { el, dispose } = mount(() => (
    <button onClick={() => setCount(count() + 1)}>{count()}</button>
  ));
  const button = el.firstChild as HTMLElement;
  expect(button.textContent).toBe("0");
  expect(fire(button, "click")).toBe(true);
  tick();
  expect(button.textContent).toBe("1");
  dispose();
  expect(el.innerHTML).toBe("");
});

test("cleanup disposes undisposed mounts", () => {
  const { el } = mount(() => <i>hi</i>);
  expect(el.textContent).toBe("hi");
  cleanup();
  expect(document.body.textContent).toBe("");
});
