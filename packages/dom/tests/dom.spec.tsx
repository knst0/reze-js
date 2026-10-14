import { signal } from "@rezejs/signals";
import { cleanup, fire, mount, tick } from "@rezejs/testing-library";
import { effect, mergeProps, omitProps, render, splitProps, type ClassValue, type JSX } from "reze-js";
import { afterEach, expect, test } from "vite-plus/test";

afterEach(cleanup);

test("static, namespaced and boolean attributes", () => {
  const [hidden, setHidden] = signal(true);
  const { el } = mount(() => (
    <div title="t" aria-label="label" bool:hidden={hidden()}>
      <svg>
        <use xlink:href="#icon" />
      </svg>
    </div>
  ));
  const div = el.firstChild as HTMLElement;
  expect(div.getAttribute("title")).toBe("t");
  expect(div.getAttribute("aria-label")).toBe("label");
  expect(div.getAttribute("hidden")).toBe("");
  setHidden(false);
  tick();
  expect(div.hasAttribute("hidden")).toBe(false);
  const svg = div.firstElementChild!;
  expect(svg.namespaceURI).toBe("http://www.w3.org/2000/svg");
  expect(svg.firstElementChild!.namespaceURI).toBe("http://www.w3.org/2000/svg");
  expect(svg.firstElementChild!.getAttributeNS("http://www.w3.org/1999/xlink", "href")).toBe("#icon");
});

const constantTruthinessRows = [
  [
    "negative zero",
    () => (
      <div>
        {/* @ts-expect-error -- Exercise untyped JavaScript truthiness. */}
        <button bool:disabled={-0} />
        {/* @ts-expect-error -- Exercise untyped JavaScript truthiness. */}
        <input type="checkbox" checked={-0} />
        <select multiple>
          {/* @ts-expect-error -- Exercise untyped JavaScript truthiness. */}
          <option selected={-0}>value</option>
        </select>
        <i data-value={-0}>{-0}</i>
      </div>
    ),
    false,
    "0",
  ],
  [
    "positive zero",
    () => (
      <div>
        {/* @ts-expect-error -- Exercise untyped JavaScript truthiness. */}
        <button bool:disabled={+0} />
        {/* @ts-expect-error -- Exercise untyped JavaScript truthiness. */}
        <input type="checkbox" checked={+0} />
        <select multiple>
          {/* @ts-expect-error -- Exercise untyped JavaScript truthiness. */}
          <option selected={+0}>value</option>
        </select>
        <i data-value={+0}>{+0}</i>
      </div>
    ),
    false,
    "0",
  ],
  [
    "arithmetic zero",
    () => (
      <div>
        {/* @ts-expect-error -- Exercise untyped JavaScript truthiness. */}
        <button bool:disabled={+(1 - 1)} />
        {/* @ts-expect-error -- Exercise untyped JavaScript truthiness. */}
        <input type="checkbox" checked={+(1 - 1)} />
        <select multiple>
          {/* @ts-expect-error -- Exercise untyped JavaScript truthiness. */}
          <option selected={+(1 - 1)}>value</option>
        </select>
        <i data-value={+(1 - 1)}>{+(1 - 1)}</i>
      </div>
    ),
    false,
    "0",
  ],
  [
    "nonzero number",
    () => (
      <div>
        {/* @ts-expect-error -- Exercise untyped JavaScript truthiness. */}
        <button bool:disabled={-1} />
        {/* @ts-expect-error -- Exercise untyped JavaScript truthiness. */}
        <input type="checkbox" checked={-1} />
        <select multiple>
          {/* @ts-expect-error -- Exercise untyped JavaScript truthiness. */}
          <option selected={-1}>value</option>
        </select>
        <i data-value={-1}>{-1}</i>
      </div>
    ),
    true,
    "-1",
  ],
  [
    "zero string",
    () => (
      <div>
        {/* @ts-expect-error -- Exercise untyped JavaScript truthiness. */}
        <button bool:disabled={"0"} />
        {/* @ts-expect-error -- Exercise untyped JavaScript truthiness. */}
        <input type="checkbox" checked={"0"} />
        <select multiple>
          {/* @ts-expect-error -- Exercise untyped JavaScript truthiness. */}
          <option selected={"0"}>value</option>
        </select>
        <i data-value={"0"}>{"0"}</i>
      </div>
    ),
    true,
    "0",
  ],
] as const;

test("constant JSX values keep boolean truthiness and rendered text in every row", () => {
  for (const [name, view, enabled, text] of constantTruthinessRows) {
    const { el } = mount(view);
    expect(el.querySelector("button")!.disabled, `${name}: disabled`).toBe(enabled);
    expect(el.querySelector("input")!.checked, `${name}: checked`).toBe(enabled);
    expect(el.querySelector("option")!.selected, `${name}: selected`).toBe(enabled);
    expect(el.querySelector("i")!.getAttribute("data-value"), `${name}: data-value`).toBe(text);
    expect(el.querySelector("i")!.textContent, `${name}: text`).toBe(text);
  }
});

test("value and checked are written as properties and survive user edits", () => {
  const [value, setValue] = signal("a");
  const [checked, setChecked] = signal(true);
  const [custom, setCustom] = signal<object>({ n: 1 });
  const { el } = mount(() => (
    <div>
      <input value={value()} />
      <input type="checkbox" checked={checked()} prop:custom={custom()} />
      <textarea value="hi" />
    </div>
  ));
  const [text, box, area] = el.firstElementChild!.children as unknown as [
    HTMLInputElement,
    HTMLInputElement & { custom: object },
    HTMLTextAreaElement,
  ];
  expect(text.value).toBe("a");
  expect(box.checked).toBe(true);
  expect(box.custom).toEqual({ n: 1 });
  expect(area.value).toBe("hi");
  text.value = "typed";
  setValue("b");
  tick();
  expect(text.value).toBe("b");
  setChecked(false);
  const next = { n: 2 };
  setCustom(next);
  tick();
  expect(box.checked).toBe(false);
  expect(box.custom).toBe(next);
});

test("select value is applied after its options are inserted", () => {
  const [value, setValue] = signal("b");
  const options = ["a", "b", "c"].map((o) => <option value={o}>{o}</option>);
  const { el } = mount(() => <select value={value()}>{options}</select>);
  const select = el.firstChild as HTMLSelectElement;
  expect(select.value).toBe("b");
  setValue("c");
  tick();
  expect(select.value).toBe("c");
});

test("class bindings toggle tokens, switch value kinds and keep shared tokens on diff", () => {
  const [active, setActive] = signal(false);
  const [n, setN] = signal(1);
  const { el } = mount(() => (
    <p>
      <b class={["btn", { active: active() }]} />
      <i class={{ negative: n() < 0, "is-zero": n() === 0 }} />
    </p>
  ));
  const b = el.firstElementChild!.children[0]!;
  const i = el.firstElementChild!.children[1]!;
  expect(b.className).toBe("btn");
  expect(i.className).toBe("");
  b.classList.add("foreign");
  setActive(true);
  tick();
  expect([...b.classList]).toEqual(["btn", "foreign", "active"]);
  setActive(false);
  tick();
  expect([...b.classList]).toEqual(["btn", "foreign"]);
  setN(-1);
  tick();
  expect(i.className).toBe("negative");
  setN(0);
  tick();
  expect(i.className).toBe("is-zero");
  {
    const [cls, setCls] = signal<ClassValue>("plain");
    const { el } = mount(() => <i class={cls()} />);
    const i = el.firstChild as HTMLElement;
    expect(i.className).toBe("plain");
    setCls(["a", "b"]);
    tick();
    expect([...i.classList].sort()).toEqual(["a", "b"]);
    setCls(["b", "c"]);
    tick();
    expect([...i.classList].sort()).toEqual(["b", "c"]);
    setCls({ d: true });
    tick();
    expect([...i.classList].sort()).toEqual(["d"]);
    setCls("plain");
    tick();
    expect(i.className).toBe("plain");
    setCls(null);
    tick();
    expect(i.hasAttribute("class")).toBe(false);
  }
  {
    const [selected, setSelected] = signal(false);
    const { el } = mount(() => <i class={{ "bg-zinc-400 text-white": !selected(), "bg-sky-400 text-white": selected() }} />);
    const i = el.firstChild as HTMLElement;
    expect([...i.classList].sort()).toEqual(["bg-zinc-400", "text-white"]);
    setSelected(true);
    tick();
    expect([...i.classList].sort()).toEqual(["bg-sky-400", "text-white"]);
  }
});

test("style accepts a string and a partly reactive object; null removes the attribute", () => {
  const [css, setCss] = signal<string | null>("color: red");
  const [width, setWidth] = signal(1);
  const { el } = mount(() => (
    <p>
      <i style={css()} />
      <b style={{ display: "block", width: `${width()}px` }} />
    </p>
  ));
  const i = el.firstElementChild!.children[0] as HTMLElement;
  const b = el.firstElementChild!.children[1] as HTMLElement;
  expect(i.style.color).toBe("red");
  expect(b.style.display).toBe("block");
  expect(b.style.width).toBe("1px");
  setCss(null);
  setWidth(2);
  tick();
  expect(i.hasAttribute("style")).toBe(false);
  expect(b.style.display).toBe("block");
  expect(b.style.width).toBe("2px");
});

test("delegated events run handlers with currentTarget, stopPropagation and [handler, data] pairs", () => {
  const [a, setA] = signal(0);
  const [b, setB] = signal(0);
  const runs: number[] = [];
  const targets: EventTarget[] = [];
  effect(() => {
    runs.push(a() + b());
  });
  const { el } = mount(() => (
    <div onClick={(e: MouseEvent) => targets.push(e.currentTarget!)}>
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
  fire(button.firstChild as HTMLElement, "click");
  tick();
  expect(runs).toEqual([0, 2]);
  expect(targets).toEqual([button, div]);
  {
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
    fire((el.firstChild as HTMLElement).firstChild as HTMLElement, "click");
    expect(log).toEqual(["inner"]);
  }
  {
    const log: string[] = [];
    const pick = (id: number, e: Event) => log.push(`${e.type} ${id}`);
    const pair: [typeof pick, number] = [pick, 2];
    const { el } = mount(() => (
      <p>
        <button onClick={[pick, 1]} />
        <input onKeyDown={pair} onInput={(e: Event) => log.push(e.type)} />
      </p>
    ));
    const [button, input] = el.firstElementChild!.children as unknown as [HTMLElement, HTMLElement];
    fire(button, "click");
    fire(input, "keydown");
    fire(input, "input");
    expect(log).toEqual(["click 1", "keydown 2", "input"]);
  }
});

test("non-delegated listeners attach directly for dblclick, on: and spread handlers", () => {
  const log: string[] = [];
  const { el } = mount(() => (
    <div onDoubleClick={() => log.push("double")} onMouseEnter={() => log.push("enter")} onScroll={() => log.push("scroll")} />
  ));
  const div = el.firstChild as HTMLElement;
  fire(div, "dblclick");
  fire(div, "mouseenter", { bubbles: false });
  div.dispatchEvent(new Event("scroll"));
  expect(log).toEqual(["double", "enter", "scroll"]);
  {
    const log: string[] = [];
    const { el } = mount(() => <i on:custom={[() => log.push("once"), { once: true }]} on:other={() => log.push("other")} />);
    const i = el.firstChild as HTMLElement;
    i.dispatchEvent(new Event("custom"));
    i.dispatchEvent(new Event("custom"));
    i.dispatchEvent(new Event("other"));
    expect(log).toEqual(["once", "other"]);
  }
  {
    const log: string[] = [];
    const { el } = mount(() => <i {...{ onDoubleClick: () => log.push("double"), onClick: () => log.push("click") }} />);
    const i = el.firstChild as HTMLElement;
    fire(i, "dblclick");
    fire(i, "click");
    expect(log).toEqual(["double", "click"]);
  }
});

test("ref assigns elements to variables, members, callbacks and forwarded component refs", () => {
  // oxlint-disable-next-line no-unassigned-vars -- the compiled ref assigns it
  let div!: HTMLDivElement;
  const refs: HTMLElement[] = [];
  const seen: Element[] = [];
  const callback = (el: Element) => seen.push(el);
  const { el } = mount(() => (
    <div ref={div}>
      <b ref={(b: HTMLElement) => seen.push(b)} />
      <i ref={refs[0]} />
      <u ref={callback} />
    </div>
  ));
  const [b, i, u] = div.children;
  expect(div).toBe(el.firstChild);
  expect(refs).toEqual([i]);
  expect(seen).toEqual([b, u]);
  {
    function Field(props: { ref?: HTMLInputElement | ((el: HTMLInputElement) => void) }) {
      return <input ref={props.ref} />;
    }
    // oxlint-disable-next-line no-unassigned-vars -- the compiled ref assigns it
    let input!: HTMLInputElement;
    const seen: Element[] = [];
    const { el } = mount(() => (
      <p>
        <Field ref={input} />
        <Field ref={(e: Element) => seen.push(e)} />
      </p>
    ));
    const [first, second] = el.firstElementChild!.children;
    expect(input).toBe(first);
    expect(seen).toEqual([second]);
  }
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
  tick();
  expect(i.getAttribute("title")).toBe("b");
  setExtra({ "data-y": "2" });
  tick();
  expect(i.hasAttribute("data-x")).toBe(false);
  expect(i.getAttribute("data-y")).toBe("2");
});

test("a dynamic native spread re-applies class, style, properties and children; later attributes win", () => {
  const [attrs, setAttrs] = signal<Record<string, unknown>>({ class: "a", style: { color: "red" }, value: "x", title: "t" });
  const [kids, setKids] = signal("one");
  const { el } = mount(() => (
    <div>
      <input {...attrs()} title="fixed" />
      <p {...attrs()}>{kids()}</p>
    </div>
  ));
  const input = el.firstElementChild!.children[0] as HTMLInputElement;
  const p = el.firstElementChild!.children[1] as HTMLElement;
  expect(input.className).toBe("a");
  expect(input.style.color).toBe("red");
  expect(input.value).toBe("x");
  expect(input.getAttribute("title")).toBe("fixed");
  expect(p.getAttribute("title")).toBe("t");
  expect(p.textContent).toBe("one");
  setAttrs({ class: "b", value: "y" });
  setKids("two");
  tick();
  expect(input.className).toBe("b");
  expect(input.hasAttribute("style")).toBe(false);
  expect(input.value).toBe("y");
  expect(input.getAttribute("title")).toBe("fixed");
  expect(p.hasAttribute("title")).toBe(false);
  expect(p.textContent).toBe("two");
});

test("omitProps drops listed keys and stays reactive", () => {
  const [label, setLabel] = signal("Save");
  let seen: Record<string, unknown> = {};
  function Button(props: { label: string; kind: string }) {
    const rest = omitProps(props, "kind") as { label: string };
    seen = rest;
    return <button>{rest.label}</button>;
  }
  const { el } = mount(() => <Button label={label()} kind="ghost" />);
  expect(el.innerHTML).toBe("<button>Save</button>");
  expect("kind" in seen).toBe(false);
  expect("label" in seen).toBe(true);
  setLabel("Send");
  tick();
  expect(el.innerHTML).toBe("<button>Send</button>");
});

test("destructured props stay reactive, including defaults and rest", () => {
  let calls = 0;
  function Link({ href, label = "home", ...rest }: { href: string; label?: string; title?: string }) {
    calls++;
    return (
      <a href={href} {...rest}>
        {label}
      </a>
    );
  }
  const [href, setHref] = signal("/a");
  const [label, setLabel] = signal<string | undefined>(undefined);
  const [title, setTitle] = signal("t1");
  const { el } = mount(() => <Link href={href()} label={label()} title={title()} />);
  expect(el.innerHTML).toBe('<a href="/a" title="t1">home</a>');
  setHref("/b");
  setLabel("B");
  setTitle("t2");
  tick();
  expect(el.innerHTML).toBe('<a href="/b" title="t2">B</a>');
  expect(calls).toBe(1);
});

test("text runs separated by elements update independently", () => {
  const [n, setN] = signal(1);
  const { el } = mount(() => (
    <div>
      {n() * 2}
      <b />
      total: {n() % 3}
      <i />
      {-n()}
    </div>
  ));
  const div = el.firstChild as HTMLElement;
  const nodes = [...div.childNodes];
  expect(div.innerHTML).toBe("2<b></b>total: 1<i></i>-1");
  setN(2);
  tick();
  expect(div.innerHTML).toBe("4<b></b>total: 2<i></i>-2");
  expect([...div.childNodes]).toEqual(nodes);
});

test("conditionals in component children and without JSX", () => {
  function Card(props: { children?: JSX.Element }) {
    return <section>{props.children}</section>;
  }
  const [ok, setOk] = signal(true);
  const { el } = mount(() => (
    <div>
      <Card>{ok() ? <b /> : null}</Card>
      <p>{ok() ? "yes" : "no"}</p>
    </div>
  ));
  expect(el.innerHTML).toBe("<div><section><b></b></section><p>yes</p></div>");
  setOk(false);
  tick();
  expect(el.innerHTML).toBe("<div><section></section><p>no</p></div>");
  setOk(true);
  tick();
  expect(el.innerHTML).toBe("<div><section><b></b></section><p>yes</p></div>");
});

test("arrays, fragments and nested getters render in order between static siblings", () => {
  const [items, setItems] = signal(["x", "y"]);
  const { el } = mount(() => (
    <ul>
      <li>first</li>
      {items().map((s) => (
        <li>{s}</li>
      ))}
      <>
        <li>middle</li>
      </>
      <li>last</li>
    </ul>
  ));
  const text = () => [...el.querySelectorAll("li")].map((li) => li.textContent);
  expect(text()).toEqual(["first", "x", "y", "middle", "last"]);
  setItems(["z"]);
  tick();
  expect(text()).toEqual(["first", "z", "middle", "last"]);
  setItems([]);
  tick();
  expect(text()).toEqual(["first", "middle", "last"]);
  setItems(["p", "q"]);
  tick();
  expect(text()).toEqual(["first", "p", "q", "middle", "last"]);
});

test("mergeProps reads literal sources in order and passes the last defined value", () => {
  function View() {
    const merged = mergeProps({ 1: 2, ["label"]: "first" }, { ["label"]: "last", [2]: 3 }) as {
      1: number;
      2: number;
      label: string;
    };
    return (
      <p>
        {merged[1]}:{merged.label}:{merged[2]}
      </p>
    );
  }
  const { el } = mount(() => <View />);
  expect(el.textContent).toBe("2:last:3");
  {
    const calls: string[] = [];
    const first = (name: string, value: number): number => {
      calls.push(name);
      return value;
    };
    function View() {
      const merged = mergeProps({ a: first("first", 1) }, {}, { a: first("second", 2), 3: first("third", 3) }) as {
        3: number;
        a: number;
      };
      return (
        <p>
          {merged.a}:{merged[3]}
        </p>
      );
    }
    const { el } = mount(() => <View />);
    expect(calls).toEqual(["first", "second", "third"]);
    expect(el.innerHTML).toBe("<p>2:3</p>");
  }
});

test("splitProps and omitProps divide literal props", () => {
  function View() {
    const [picked, rest] = splitProps({ x: 1, y: 2 }, ["x"]);
    const kept = omitProps({ x: 1, y: 2 }, "x") as { y: number };
    return (
      <p>
        {picked.x}:{rest.y}:{kept.y}
      </p>
    );
  }
  const { el } = mount(() => <View />);
  expect(el.textContent).toBe("1:2:2");
});

test("destructured defaults stay lazy and see earlier props", () => {
  let calls = 0;
  const fallback = (): string => {
    calls++;
    return "home";
  };
  function Link({ href, label = fallback }: { href: string; label?: string | (() => string) }) {
    return <a href={href}>{label}</a>;
  }
  function Card({ user: { name } }: { user: { name: string } }) {
    return <b>{name}</b>;
  }
  const [href, setHref] = signal("/a");
  const [user, setUser] = signal({ name: "ann" });
  const { el } = mount(() => (
    <>
      <Link href={href()} />
      <Card user={user()} />
    </>
  ));
  expect(el.innerHTML).toBe('<a href="/a">home</a><b>ann</b>');
  expect(calls).toBe(1);
  setHref("/b");
  setUser({ name: "bob" });
  tick();
  expect(el.innerHTML).toBe('<a href="/b">home</a><b>bob</b>');
  const provided = mount(() => <Link href="/c" label="here" />);
  expect(provided.el.innerHTML).toBe('<a href="/c">here</a>');
  expect(calls).toBe(1);
});

test("nested children win over the children attribute", () => {
  const [kids, setKids] = signal("attr");
  const fromAttr = mount(() => <div children={kids()} />);
  expect((fromAttr.el.firstChild as HTMLElement).innerHTML).toBe("attr");
  // @ts-expect-error -- nested children intentionally override the attribute.
  const nested = mount(() => <div children={kids()}>nested</div>);
  expect((nested.el.firstChild as HTMLElement).innerHTML).toBe("nested");
  setKids("changed");
  tick();
  expect((fromAttr.el.firstChild as HTMLElement).innerHTML).toBe("changed");
  expect((nested.el.firstChild as HTMLElement).innerHTML).toBe("nested");
});

function freshTarget(key: string): unknown {
  const merged = mergeProps({ [key]: 7 }, { [key]: new.target }) as Record<string, unknown>;
  return merged[key];
}

test("refused default positions keep working arguments", () => {
  function Self({ a = arguments[0] }: { a?: unknown }) {
    return <p>{a === undefined ? "none" : "props"}</p>;
  }
  const { el } = mount(() => <Self />);
  expect(el.innerHTML).toBe("<p>props</p>");
});

const KEY = "a";

test("refused props shapes still run as ordinary code", () => {
  let bumps = 0;
  const bump = (): number => {
    bumps++;
    return bumps;
  };
  function ByKey({ [KEY]: v }: { a: number }) {
    return <i>{v}</i>;
  }
  function WithCall({ a = bump() }: { a?: number }) {
    return <i>{a}</i>;
  }
  function Deep({ a: { b } = { b: "deep" } }: { a?: { b: string } }) {
    return <i>{b}</i>;
  }
  function RestDeep({ a: { ...kept } }: { a: { x: number } }) {
    return <i>{kept.x}</i>;
  }
  function Grown({ a }: { a: number }) {
    a = a + 1;
    return <i>{a}</i>;
  }
  function FirstOfPair({ a }: { a: number }, _ref?: unknown) {
    return <i>{a}</i>;
  }
  function Single({ a: [b] }: { a: [number] }) {
    return <i>{b}</i>;
  }
  const { el } = mount(() => (
    <p>
      <ByKey a={1} />
      <WithCall />
      <Deep />
      <RestDeep a={{ x: 2 }} />
      <Grown a={3} />
      <FirstOfPair a={4} />
      <Single a={[5]} />
    </p>
  ));
  expect(el.innerHTML).toBe("<p><i>1</i><i>1</i><i>deep</i><i>2</i><i>4</i><i>4</i><i>5</i></p>");
  expect(bumps).toBe(1);
});

test("a refused generator shape still yields its view", () => {
  function* Gen({ a }: { a: number }) {
    yield <p>{a}</p>;
  }
  const { el } = mount(() => Gen({ a: 7 }).next().value!);
  expect(el.innerHTML).toBe("<p>7</p>");
});
