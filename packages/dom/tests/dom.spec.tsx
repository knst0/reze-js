import { effect, flushSync, onCleanup, signal } from "@rezejs/signals";
import { cleanup, mount } from "@rezejs/test-utils";
import { afterEach, expect, test } from "vitest";

import { type ClassValue, className, mergeProps, use } from "../src";

afterEach(cleanup);

test("text and attribute bindings update in place", () => {
  const [name, setName] = signal("a");
  const { el } = mount(() => <p title={name()}>hi {name()}!</p>);
  const p = el.firstChild as HTMLElement;
  const text = p.childNodes[1];
  setName("b");
  flushSync();
  // `<!---->` is the compiler's insertion marker.
  expect(el.innerHTML).toBe('<p title="b">hi b<!---->!</p>');
  expect(el.firstChild).toBe(p);
  expect(p.childNodes[1]).toBe(text);
});

test("null, undefined and false remove the attribute", () => {
  const [v, setV] = signal<string | false | null>("x");
  const { el } = mount(() => <i data-v={v()} />);
  const i = el.firstChild as HTMLElement;
  expect(i.getAttribute("data-v")).toBe("x");
  setV(false);
  flushSync();
  expect(i.hasAttribute("data-v")).toBe(false);
  setV("y");
  flushSync();
  setV(null);
  flushSync();
  expect(i.hasAttribute("data-v")).toBe(false);
});

test("class accepts a string or a toggle object", () => {
  const [on, setOn] = signal(true);
  const [cls, setCls] = signal<string | Record<string, boolean>>("a b");
  const { el } = mount(() => <i class={[cls(), { on: on() }]} />);
  const i = el.firstChild as HTMLElement;
  expect(i.className).toBe("a b on");
  setOn(false);
  flushSync();
  expect(i.className).toBe("a b");
  setCls({ "x y": true, z: false });
  flushSync();
  expect([...i.classList].sort()).toEqual(["x", "y"]);
});

test("style objects set, update and drop properties", () => {
  const [s, setS] = signal<Record<string, string | undefined>>({
    color: "red",
    "margin-top": "1px",
  });
  const { el } = mount(() => <i style={s()} />);
  const i = el.firstChild as HTMLElement;
  expect(i.style.color).toBe("red");
  setS({ color: "blue" });
  flushSync();
  expect(i.style.color).toBe("blue");
  expect(i.style.marginTop).toBe("");
});

test("all-literal style and class objects compile to static attributes", () => {
  // The compiler folds these into the template (C09/C10): no runtime call,
  // so the parsed attributes must already carry the values.
  const { el } = mount(() => (
    <i style={{ color: "red", "margin-top": "1px" }} class={{ a: true, b: false }} />
  ));
  const i = el.firstChild as HTMLElement;
  expect(i.style.color).toBe("red");
  expect(i.style.marginTop).toBe("1px");
  expect(i.className).toBe("a");
});

test("delegated handlers coalesce writes and see the declaring element as currentTarget", () => {
  const [a, setA] = signal(0);
  const [b, setB] = signal(0);
  const runs: number[] = [];
  const targets: EventTarget[] = [];
  effect(() => {
    runs.push(a() + b());
  });
  const { el } = mount(() => (
    <div
      onClick={(e: MouseEvent) => {
        targets.push(e.currentTarget!);
      }}
    >
      <button
        onClick={(e: MouseEvent) => {
          targets.push(e.currentTarget!);
          setA(1);
          setB(1);
        }}
      >
        <span>go</span>
      </button>
    </div>
  ));
  const div = el.firstChild as HTMLElement;
  const button = div.firstChild as HTMLElement;
  (button.firstChild as HTMLElement).click();
  flushSync();
  expect(runs).toEqual([0, 2]);
  expect(targets).toEqual([button, div]);
});

test("stopPropagation stops delegated bubbling", () => {
  const log: string[] = [];
  const { el } = mount(() => (
    <div onClick={() => log.push("outer")}>
      <button
        onClick={(e: MouseEvent) => {
          log.push("inner");
          e.stopPropagation();
        }}
      />
    </div>
  ));
  ((el.firstChild as HTMLElement).firstChild as HTMLElement).click();
  expect(log).toEqual(["inner"]);
});

test("on: attaches a direct listener; [handler, options] passes listener options", () => {
  const log: string[] = [];
  const { el } = mount(() => (
    <i on:custom={[() => log.push("once"), { once: true }]} on:other={() => log.push("other")} />
  ));
  const i = el.firstChild as HTMLElement;
  i.dispatchEvent(new Event("custom"));
  i.dispatchEvent(new Event("custom"));
  i.dispatchEvent(new Event("other"));
  expect(log).toEqual(["once", "other"]);
});

test("spread onDoubleClick listens for dblclick", () => {
  const log: string[] = [];
  const { el } = mount(() => <i {...{ onDoubleClick: () => log.push("double") }} />);
  const i = el.firstChild as HTMLElement;
  i.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
  expect(log).toEqual(["double"]);
});

test("components run once; props read lazily keep reactivity", () => {
  let calls = 0;
  function Label(props: { text: string }) {
    calls++;
    return <b>{props.text}</b>;
  }
  const [t, setT] = signal("a");
  const { el } = mount(() => <Label text={t()} />);
  setT("b");
  flushSync();
  setT("c");
  flushSync();
  expect(el.innerHTML).toBe("<b>c</b>");
  expect(calls).toBe(1);
});

test("a conditional branch is disposed when it switches out", () => {
  const log: string[] = [];
  function Child(props: { name: string }) {
    onCleanup(() => log.push("cleanup " + props.name));
    return <em>{props.name}</em>;
  }
  const [on, setOn] = signal(true);
  const { el, dispose } = mount(() => <div>{on() ? <Child name="a" /> : <Child name="b" />}</div>);
  expect(el.innerHTML).toBe("<div><em>a</em></div>");
  setOn(false);
  flushSync();
  expect(el.innerHTML).toBe("<div><em>b</em></div>");
  expect(log).toEqual(["cleanup a"]);
  dispose();
  expect(log).toEqual(["cleanup a", "cleanup b"]);
  expect(el.innerHTML).toBe("");
});

test("arrays, fragments and nested getters render in order between static siblings", () => {
  const [items, setItems] = signal(["x", "y"]);
  const { el } = mount(() => (
    <ul>
      <li>first</li>
      {items().map((s) => (
        <li>{s}</li>
      ))}
      <li>last</li>
    </ul>
  ));
  const text = () => [...el.querySelectorAll("li")].map((li) => li.textContent);
  expect(text()).toEqual(["first", "x", "y", "last"]);
  setItems(["z"]);
  flushSync();
  expect(text()).toEqual(["first", "z", "last"]);
  setItems([]);
  flushSync();
  expect(text()).toEqual(["first", "last"]);
  setItems(["p", "q"]);
  flushSync();
  expect(text()).toEqual(["first", "p", "q", "last"]);
});

test("spread applies reactive props, including ones from a function source", () => {
  const [title, setTitle] = signal("a");
  const [extra, setExtra] = signal<Record<string, string>>({ "data-x": "1" });
  const { el } = mount(() => (
    <i
      {...mergeProps(
        {
          get title() {
            return title();
          },
        },
        extra,
      )}
    />
  ));
  const i = el.firstChild as HTMLElement;
  expect(i.getAttribute("title")).toBe("a");
  expect(i.getAttribute("data-x")).toBe("1");
  setTitle("b");
  flushSync();
  expect(i.getAttribute("title")).toBe("b");
  setExtra({ "data-y": "2" });
  flushSync();
  expect(i.hasAttribute("data-x")).toBe(false);
  expect(i.getAttribute("data-y")).toBe("2");
});

test("class accepts arrays mixing strings, objects and nested arrays", () => {
  const { el } = mount(() => (
    <i
      class={[
        "a",
        false,
        "b",
        { c: true, d: false },
        ["e", ["f", { g: true }]],
        null,
        undefined,
        "",
        0,
      ]}
    />
  ));
  const i = el.firstChild as HTMLElement;
  expect([...i.classList].sort()).toEqual(["0", "a", "b", "c", "e", "f", "g"]);
});

test("reactive class arrays and objects drop stale classes", () => {
  const [cls, setCls] = signal<ClassValue>(["a", "b"]);
  const { el } = mount(() => <i class={cls()} />);
  const i = el.firstChild as HTMLElement;
  expect([...i.classList].sort()).toEqual(["a", "b"]);
  setCls(["b", "c"]);
  flushSync();
  expect([...i.classList].sort()).toEqual(["b", "c"]);
  setCls({ d: true });
  flushSync();
  expect([...i.classList].sort()).toEqual(["d"]);
  setCls(null);
  flushSync();
  expect(i.hasAttribute("class")).toBe(false);
});

test("className swaps classes across string, object and array with explicit prev", () => {
  const i = document.createElement("i");
  className(i, "old");
  expect(i.className).toBe("old");
  className(i, { bright: true, fresh: true }, "old");
  expect([...i.classList].sort()).toEqual(["bright", "fresh"]);
  className(i, ["next", { shiny: true }], { bright: true, fresh: true });
  expect([...i.classList].sort()).toEqual(["next", "shiny"]);
  className(i, "plain", ["next", { shiny: true }]);
  expect(i.className).toBe("plain");
});

test("composite object keys keep shared tokens on diff", () => {
  const i = document.createElement("i");
  const first = { "bg-zinc-400 text-white": true, "bg-sky-400 text-white": true };
  className(i, first);
  expect(i.classList.contains("text-white")).toBe(true);
  className(i, { "bg-zinc-400 text-white": false, "bg-sky-400 text-white": true }, first);
  expect(i.classList.contains("text-white")).toBe(true);
  expect(i.classList.contains("bg-zinc-400")).toBe(false);
  expect(i.classList.contains("bg-sky-400")).toBe(true);
});

test("className accepts arrays and drops stale keys on update", () => {
  const i = document.createElement("i");
  const first: ClassValue[] = ["a", { b: true, c: false }];
  className(i, first);
  expect([...i.classList].sort()).toEqual(["a", "b"]);
  className(i, ["b", "d"], first);
  expect([...i.classList].sort()).toEqual(["b", "d"]);
});

test("use() infers the element type from ref callbacks (D10)", () => {
  const div = document.createElement("div");
  let tagged = "";
  use((el: HTMLDivElement) => {
    tagged = el.tagName;
  }, div);
  expect(tagged).toBe("DIV");
});
