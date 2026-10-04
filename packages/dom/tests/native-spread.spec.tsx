import { cleanup, mount, tick } from "@rezejs/testing-library";
import { signal } from "@rezejs/signals";
import { afterEach, expect, test } from "vitest";

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

test("a closed-shape spread skips setter writes for entries whose value did not change", () => {
  const writes: string[] = [];
  const [first, setFirst] = signal("a");
  const [second, setSecond] = signal("b");
  mount(() => (
    <div
      ref={(node) => traceProps(node as HTMLElement, writes, "pa", "pb")}
      {...{ "prop:pa": `a=${first()}`, "prop:pb": `b=${second()}` }}
    />
  ));
  expect(writes).toEqual(["write:pa=a=a", "write:pb=b=b"]);
  writes.length = 0;
  setFirst("x");
  tick();
  expect(writes).toEqual(["write:pa=a=x"]);
  setSecond("y");
  tick();
  expect(writes).toEqual(["write:pa=a=x", "write:pb=b=y"]);
});

test("disposing a closed-shape spread stops its subscriptions", () => {
  const [title, setTitle] = signal("a");
  const { el, dispose } = mount(() => <div {...{ title: `t=${title()}`, "data-n": `n=${title()}` }} />);
  const div = el.firstChild as HTMLElement;
  expect(div.getAttribute("title")).toBe("t=a");
  dispose();
  setTitle("b");
  tick();
  expect(div.getAttribute("title")).toBe("t=a");
  expect(div.getAttribute("data-n")).toBe("n=a");
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

test("alias and proxy spread sources keep generic spread semantics", () => {
  const [title, setTitle] = signal("proxied");
  const aliased = { title: "aliased", "data-n": "2" };
  const proxied = new Proxy(
    { title: "unused" },
    {
      get(target, key, receiver) {
        return key === "title" ? title() : Reflect.get(target, key, receiver);
      },
    },
  );
  const { el } = mount(() => (
    <div>
      <i {...aliased} />
      <b {...proxied} />
    </div>
  ));
  const [i, b] = el.firstElementChild!.children as unknown as [HTMLElement, HTMLElement];
  expect(i.getAttribute("title")).toBe("aliased");
  expect(i.getAttribute("data-n")).toBe("2");
  expect(b.getAttribute("title")).toBe("proxied");
  setTitle("updated");
  tick();
  expect(b.getAttribute("title")).toBe("updated");
});

test("a multi-bind group evaluates every value before any target write", () => {
  const log: string[] = [];
  const seen = (name: string, value: string): string => {
    log.push(`eval:${name}=${value}`);
    return value;
  };
  const [first, setFirst] = signal("a");
  const [second, setSecond] = signal("b");
  mount(() => (
    <div
      ref={(node) => traceProps(node as HTMLElement, log, "title", "tip")}
      prop:title={seen("title", `${first()}:${second()}`)}
      prop:tip={seen("tip", `${first()}-${second()}`)}
    />
  ));
  expect(log).toEqual(["eval:title=a:b", "eval:tip=a-b", "write:title=a:b", "write:tip=a-b"]);
  log.length = 0;
  setFirst("x");
  setSecond("y");
  tick();
  expect(log).toEqual(["eval:title=x:y", "eval:tip=x-y", "write:title=x:y", "write:tip=x-y"]);
});
