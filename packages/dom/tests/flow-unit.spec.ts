import { flush, signal } from "@rezejs/signals";
import { renderEffect } from "@rezejs/signals/render";
import { afterEach, expect, test } from "vitest";

import { render } from "../src/component";
import { branch, choose } from "../src/flow";

let unmount: (() => void) | undefined;

afterEach(() => {
  unmount?.();
  unmount = undefined;
});

function mount(code: () => unknown): HTMLElement {
  const container = document.createElement("div");
  unmount = render(code as () => Node, container);
  return container;
}

function element(tag: string, text: () => unknown): HTMLElement {
  const el = document.createElement(tag);
  renderEffect(() => {
    el.textContent = String(text());
  });
  return el;
}

test("branch rebuilds the child only when truthiness flips", () => {
  const [n, setN] = signal(1);
  let builds = 0;
  const container = mount(() =>
    branch(
      n,
      (value) => {
        builds++;
        return element("p", value);
      },
      () => element("i", () => "none"),
    ),
  );
  const shown = container.firstChild;
  expect(container.innerHTML).toBe("<p>1</p>");

  setN(2);
  flush();
  expect(builds).toBe(1);
  expect(container.firstChild).toBe(shown);
  expect(container.innerHTML).toBe("<p>2</p>");

  setN(0);
  flush();
  setN(3);
  flush();
  expect(builds).toBe(2);
  expect(container.innerHTML).toBe("<p>3</p>");
});

test("branch shows the fallback while when is falsy, and nothing without one", () => {
  const [on, setOn] = signal<string | null>(null);
  const withFallback = mount(() =>
    branch(
      on,
      (value) => element("p", value),
      () => element("i", () => "guest"),
    ),
  );
  expect(withFallback.innerHTML).toBe("<i>guest</i>");
  setOn("ann");
  flush();
  expect(withFallback.innerHTML).toBe("<p>ann</p>");
  setOn(null);
  flush();
  expect(withFallback.innerHTML).toBe("<i>guest</i>");

  unmount!();
  const bare = mount(() => branch(on, (value) => element("p", value)));
  expect(bare.innerHTML).toBe("");
});

test("the branch value getter tracks when", () => {
  const [user, setUser] = signal({ name: "ann" });
  const reads: string[] = [];
  mount(() =>
    branch(user, (value) =>
      element("b", () => {
        reads.push(value().name);
        return value().name;
      }),
    ),
  );
  setUser({ name: "bob" });
  flush();
  expect(reads).toEqual(["ann", "bob"]);
});

test("choose rebuilds only when the chosen index changes", () => {
  const [a, setA] = signal(1);
  const [b, setB] = signal(0);
  const builds: string[] = [];
  const container = mount(() =>
    choose(
      [a, b],
      [
        (value) => {
          builds.push("a");
          return element("u", value);
        },
        (value) => {
          builds.push("b");
          return element("s", value);
        },
      ],
      () => {
        builds.push("fallback");
        return element("i", () => "none");
      },
    ),
  );
  expect(container.innerHTML).toBe("<u>1</u>");

  setA(2);
  setB(5);
  flush();
  expect(builds).toEqual(["a"]);
  expect(container.innerHTML).toBe("<u>2</u>");

  setA(0);
  flush();
  expect(builds).toEqual(["a", "b"]);
  expect(container.innerHTML).toBe("<s>5</s>");

  setB(0);
  flush();
  expect(builds).toEqual(["a", "b", "fallback"]);
  expect(container.innerHTML).toBe("<i>none</i>");
});
