import { effect, flush, onCleanup, signal } from "@rezejs/signals";
import { expect, test } from "vite-plus/test";

import { render } from "../src/component";
import { hotComponent } from "../src/hot";

test("registering an id again re-renders mounted instances in place", () => {
  const cleaned: string[] = [];
  const V1 = (props: { label: string }) => {
    onCleanup(() => cleaned.push("v1"));
    const el = document.createElement("p");
    el.textContent = `v1 ${props.label}`;
    return el;
  };
  const V2 = (props: { label: string }) => {
    const el = document.createElement("p");
    el.textContent = `v2 ${props.label}`;
    return el;
  };

  const Hot = hotComponent({}, "b.tsx#Label", V1);
  const container = document.createElement("div");
  const dispose = render(() => Hot({ label: "x" }), container);
  expect(container.innerHTML).toBe("<p>v1 x</p>");

  expect(hotComponent({}, "b.tsx#Label", V2)).toBe(Hot);
  flush();
  expect(container.innerHTML).toBe("<p>v2 x</p>");
  expect(cleaned).toEqual(["v1"]);
  dispose();
});

test("swap preserves a mutated signal value and ignores the new initializer", () => {
  let bump!: () => void;
  const V1 = () => {
    const [count, setCount] = signal(0);
    bump = () => setCount((n) => n + 1);
    const el = document.createElement("p");
    effect(() => {
      el.textContent = `v1 ${count()}`;
    });
    return el;
  };
  const V2 = () => {
    const [count, setCount] = signal(100);
    bump = () => setCount((n) => n + 1);
    const el = document.createElement("p");
    effect(() => {
      el.textContent = `v2 ${count()}`;
    });
    return el;
  };

  const Hot = hotComponent({}, "b.tsx#Counter", V1);
  const container = document.createElement("div");
  const dispose = render(() => Hot({}), container);
  flush();
  expect(container.innerHTML).toBe("<p>v1 0</p>");
  bump();
  flush();
  expect(container.innerHTML).toBe("<p>v1 1</p>");

  hotComponent({}, "b.tsx#Counter", V2);
  flush();
  expect(container.innerHTML).toBe("<p>v2 1</p>");
  bump();
  flush();
  expect(container.innerHTML).toBe("<p>v2 2</p>");
  dispose();
});

test("two mounted instances of the same component preserve state independently", () => {
  const bumps: Record<string, () => void> = {};
  const make = (version: string) => (props: { label: string }) => {
    const [count, setCount] = signal(0);
    bumps[props.label] = () => setCount((n) => n + 1);
    const el = document.createElement("p");
    effect(() => {
      el.textContent = `${version} ${props.label} ${count()}`;
    });
    return el;
  };

  const Hot = hotComponent({}, "b.tsx#Pair", make("v1"));
  const first = document.createElement("div");
  const second = document.createElement("div");
  const disposeFirst = render(() => Hot({ label: "a" }), first);
  const disposeSecond = render(() => Hot({ label: "b" }), second);
  flush();
  bumps["a"]();
  bumps["a"]();
  bumps["b"]();
  flush();
  expect(first.innerHTML).toBe("<p>v1 a 2</p>");
  expect(second.innerHTML).toBe("<p>v1 b 1</p>");

  hotComponent({}, "b.tsx#Pair", make("v2"));
  flush();
  expect(first.innerHTML).toBe("<p>v2 a 2</p>");
  expect(second.innerHTML).toBe("<p>v2 b 1</p>");
  disposeFirst();
  disposeSecond();
});

test("a changed initializer type resets that position while a later signal keeps its value", () => {
  let mutate!: () => void;
  const V1 = () => {
    const [count, setCount] = signal(0);
    const [label, setLabel] = signal("a");
    mutate = () => {
      setCount(42);
      setLabel("kept");
    };
    const el = document.createElement("p");
    effect(() => {
      el.textContent = `v1 ${count()} ${label()}`;
    });
    return el;
  };
  const V2 = () => {
    const [count] = signal("reset");
    const [label] = signal("default");
    const el = document.createElement("p");
    effect(() => {
      el.textContent = `v2 ${count()} ${label()}`;
    });
    return el;
  };

  const Hot = hotComponent({}, "b.tsx#Mixed", V1);
  const container = document.createElement("div");
  const dispose = render(() => Hot({}), container);
  flush();
  mutate();
  flush();
  expect(container.innerHTML).toBe("<p>v1 42 kept</p>");

  hotComponent({}, "b.tsx#Mixed", V2);
  flush();
  expect(container.innerHTML).toBe("<p>v2 reset kept</p>");
  dispose();
});
