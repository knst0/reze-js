import { signal } from "@rezejs/signals";
import { cleanup, mount, tick } from "@rezejs/testing-library";
import { signal as sig } from "reze-js";
import { afterEach, expect, test } from "vite-plus/test";

afterEach(cleanup);

function traceProps(target: HTMLElement, log: string[], ...keys: string[]): void {
  for (const key of keys) {
    let current: unknown;
    Object.defineProperty(target, key, {
      configurable: true,
      enumerable: true,
      get: () => current,
      set: (next: unknown) => {
        log.push(`write:${key}=${String(next)}`);
        current = next;
      },
    });
  }
}

test("a closed-shape spread evaluates entries in source order, writing each before the next", () => {
  const log: string[] = [];
  const seen = (name: string, value: string): string => {
    log.push(`eval:${name}=${value}`);
    return value;
  };
  const [first, setFirst] = signal("a");
  const [second, setSecond] = signal("b");
  mount(() => (
    <div
      ref={(node) => traceProps(node as HTMLElement, log, "pa", "pb", "pc")}
      {...{
        "prop:pa": `${seen("pa", first())}`,
        "prop:pb": `${seen("pb", second())}`,
        "prop:pc": `${seen("pc", first())}${seen("pd", second())}`,
      }}
    />
  ));
  expect(log).toEqual(["eval:pa=a", "write:pa=a", "eval:pb=b", "write:pb=b", "eval:pc=a", "eval:pd=b", "write:pc=ab"]);
  log.length = 0;
  setFirst("x");
  setSecond("y");
  tick();
  expect(log).toEqual(["eval:pa=x", "write:pa=x", "eval:pb=y", "write:pb=y", "eval:pc=x", "eval:pd=y", "write:pc=xy"]);
});

test("plain spread values are captured once and reused across reruns", () => {
  const [n, setN] = signal(0);
  const { el } = mount(() => (
    <div
      {...{
        title: `n=${n()}`,
        "data-fixed": "fixed",
        "prop:config": { mode: "fast", retries: 1 + 2 },
        "prop:handle": () => n(),
        class: { held: true, dropped: false },
      }}
    />
  ));
  const div = el.firstChild as HTMLElement & Record<string, unknown>;
  const config = div.config;
  const handle = div.handle;
  expect(div.getAttribute("title")).toBe("n=0");
  expect(div.getAttribute("data-fixed")).toBe("fixed");
  expect(div.className).toBe("held");
  setN(1);
  tick();
  setN(2);
  tick();
  expect(div.getAttribute("title")).toBe("n=2");
  expect(div.getAttribute("data-fixed")).toBe("fixed");
  expect(div.config).toBe(config);
  expect(div.handle).toBe(handle);
  expect(div.className).toBe("held");
});

test("a closed-shape spread drops style properties the next value omits", () => {
  const [alt, setAlt] = signal(false);
  const { el } = mount(() => <div {...{ style: alt() ? { color: "green" } : { color: "red", "margin-top": "1px" } }} />);
  const div = el.firstChild as HTMLElement;
  expect(div.style.color).toBe("red");
  expect(div.style.marginTop).toBe("1px");
  setAlt(true);
  tick();
  expect(div.style.color).toBe("green");
  expect(div.style.marginTop).toBe("");
  setAlt(false);
  tick();
  expect(div.style.color).toBe("red");
  expect(div.style.marginTop).toBe("1px");
});

test("a closed-shape spread normalizes array and object class values", () => {
  const [on, setOn] = signal(false);
  const [flat, setFlat] = signal(false);
  const { el } = mount(() => <div {...{ class: flat() ? "flat" : ["base", { extra: on() }] }} />);
  const div = el.firstChild as HTMLElement;
  expect(div.className).toBe("base");
  setOn(true);
  tick();
  expect([...div.classList].sort()).toEqual(["base", "extra"]);
  setOn(false);
  tick();
  expect(div.className).toBe("base");
  setFlat(true);
  tick();
  expect(div.className).toBe("flat");
  setFlat(false);
  tick();
  expect(div.className).toBe("base");
});

test("a closed-shape spread honors prop:, attr: and bool: prefixes", () => {
  const [hint, setHint] = signal("one");
  const [off, setOff] = signal(false);
  const { el } = mount(() => (
    <button {...{ "prop:tag": `tag-${hint()}`, "attr:data-hint": `hint-${hint()}`, "bool:disabled": off() ? true : false }} />
  ));
  const button = el.firstChild as HTMLButtonElement & Record<string, unknown>;
  expect(button.tag).toBe("tag-one");
  expect(button.getAttribute("data-hint")).toBe("hint-one");
  expect(button.disabled).toBe(false);
  setHint("two");
  setOff(true);
  tick();
  expect(button.tag).toBe("tag-two");
  expect(button.getAttribute("data-hint")).toBe("hint-two");
  expect(button.disabled).toBe(true);
  setOff(false);
  tick();
  expect(button.disabled).toBe(false);
  expect(button.hasAttribute("disabled")).toBe(false);
});

test("duplicate spread keys keep later-wins semantics through the generic path", () => {
  const [name, setName] = signal("a");
  const { el: fixed } = mount(() => <div {...{ title: `spread-${name()}` }} title="fixed" />);
  const { el: layered } = mount(() => <div {...{ title: "first" }} {...{ title: `second-${name()}` }} />);
  expect((fixed.firstChild as HTMLElement).getAttribute("title")).toBe("fixed");
  expect((layered.firstChild as HTMLElement).getAttribute("title")).toBe("second-a");
  setName("b");
  tick();
  expect((fixed.firstChild as HTMLElement).getAttribute("title")).toBe("fixed");
  expect((layered.firstChild as HTMLElement).getAttribute("title")).toBe("second-b");
});

test("a later maybe-undefined key falls back without clobbering the spread", () => {
  const [ok, setOk] = signal(true);
  const { el } = mount(() => <div {...{ title: "spread", "data-x": "1" }} title={ok() ? "late" : undefined} />);
  const div = el.firstChild as HTMLElement;
  expect(div.getAttribute("title")).toBe("late");
  expect(div.getAttribute("data-x")).toBe("1");
  setOk(false);
  tick();
  expect(div.getAttribute("title")).toBe("spread");
  expect(div.getAttribute("data-x")).toBe("1");
  setOk(true);
  tick();
  expect(div.getAttribute("title")).toBe("late");
});
