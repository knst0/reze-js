import { cleanup, mount, tick } from "@rezejs/testing-library";
import { For, Match, onCleanup, Show, Switch } from "reze-js";
import { signal } from "@rezejs/signals";
import { afterEach, expect, test } from "vitest";

afterEach(cleanup);

const texts = (el: Element) => [...el.children].map((child) => child.textContent);

test("Show keeps its branch while when stays truthy and passes the value as a tracked getter", () => {
  let builds = 0;
  const [user, setUser] = signal<{ name: string } | null>({ name: "a" });
  const { el } = mount(() => (
    <Show when={user()} fallback={<i>none</i>}>
      {(u) => {
        builds++;
        return <b>{u().name}</b>;
      }}
    </Show>
  ));
  const b = el.firstChild;
  expect(el.innerHTML).toBe("<b>a</b>");
  setUser({ name: "b" });
  tick();
  expect(el.innerHTML).toBe("<b>b</b>");
  expect(el.firstChild).toBe(b);
  expect(builds).toBe(1);
  setUser(null);
  tick();
  expect(el.innerHTML).toBe("<i>none</i>");
  setUser({ name: "c" });
  tick();
  expect(el.innerHTML).toBe("<b>c</b>");
  expect(builds).toBe(2);
});

test("Show with element children rebuilds only when truthiness flips, between static siblings", () => {
  let builds = 0;
  function Big(props: { n: number }) {
    builds++;
    return <b>{props.n}</b>;
  }
  const [n, setN] = signal(10);
  const { el } = mount(() => (
    <div>
      x
      <Show when={n() >= 10} fallback={<i>small</i>}>
        <Big n={n()} />
      </Show>
      y
    </div>
  ));
  const b = el.querySelector("b");
  expect(el.innerHTML).toBe("<div>x<b>10</b><!---->y</div>");
  setN(11);
  tick();
  expect(el.innerHTML).toBe("<div>x<b>11</b><!---->y</div>");
  expect(el.querySelector("b")).toBe(b);
  expect(builds).toBe(1);
  setN(3);
  tick();
  expect(el.innerHTML).toBe("<div>x<i>small</i><!---->y</div>");
  setN(12);
  tick();
  expect(el.innerHTML).toBe("<div>x<b>12</b><!---->y</div>");
  expect(builds).toBe(2);
});

test("Show renders several children in order and nothing without a fallback", () => {
  const [user, setUser] = signal<{ name: string } | undefined>(undefined);
  const { el } = mount(() => (
    <p>
      <Show when={user()}>
        {user()!.name}
        <b />
        <i />
      </Show>
    </p>
  ));
  expect(el.innerHTML).toBe("<p></p>");
  setUser({ name: "ann" });
  tick();
  expect(el.innerHTML).toBe("<p>ann<b></b><i></i></p>");
  setUser({ name: "bob" });
  tick();
  expect(el.innerHTML).toBe("<p>bob<b></b><i></i></p>");
  setUser(undefined);
  tick();
  expect(el.innerHTML).toBe("<p></p>");
});

test("Show disposes the branch it switches away from, at the top level and inside an element", () => {
  const log: string[] = [];
  function Child(props: { name: string }) {
    onCleanup(() => log.push(props.name));
    return <b />;
  }
  const [on, setOn] = signal(true);
  const top = mount(() => (
    <Show when={on()} fallback={<Child name="top fallback" />}>
      <Child name="top child" />
    </Show>
  ));
  const nested = mount(() => (
    <div>
      <Show when={on()} fallback={<Child name="nested fallback" />}>
        <Child name="nested child" />
      </Show>
    </div>
  ));
  setOn(false);
  tick();
  expect(log.sort()).toEqual(["nested child", "top child"]);
  log.length = 0;
  top.dispose();
  nested.dispose();
  expect(log).toEqual(["top fallback", "nested fallback"]);
});

test("Switch renders the first truthy Match and rebuilds only when the choice changes", () => {
  let builds = 0;
  function Small() {
    builds++;
    return <b>small</b>;
  }
  const [n, setN] = signal(1);
  const { el } = mount(() => (
    <div>
      <Switch fallback={<i>zero</i>}>
        <Match when={n() > 10}>
          <b>big</b>
        </Match>
        <Match when={n() > 0}>
          <Small />
        </Match>
      </Switch>
    </div>
  ));
  expect(el.innerHTML).toBe("<div><b>small</b></div>");
  setN(2);
  tick();
  expect(builds).toBe(1);
  setN(20);
  tick();
  expect(el.innerHTML).toBe("<div><b>big</b></div>");
  setN(0);
  tick();
  expect(el.innerHTML).toBe("<div><i>zero</i></div>");
  setN(5);
  tick();
  expect(el.innerHTML).toBe("<div><b>small</b></div>");
  expect(builds).toBe(2);
});

test("a function child of Match receives its when as a tracked getter", () => {
  let builds = 0;
  const [a, setA] = signal(false);
  const [b, setB] = signal("x");
  const { el } = mount(() => (
    <Switch>
      <Match when={a()}>
        <i>a</i>
      </Match>
      <Match when={b()}>
        {(value) => {
          builds++;
          return <u>{value()}</u>;
        }}
      </Match>
    </Switch>
  ));
  expect(el.innerHTML).toBe("<u>x</u>");
  setB("y");
  tick();
  expect(el.innerHTML).toBe("<u>y</u>");
  expect(builds).toBe(1);
  setA(true);
  tick();
  expect(el.innerHTML).toBe("<i>a</i>");
  setB("");
  setA(false);
  tick();
  expect(el.innerHTML).toBe("");
});

test("For rows follow items by identity: nodes move instead of being rebuilt", () => {
  const [items, setItems] = signal(["a", "b", "c"]);
  const { el } = mount(() => <For each={items()}>{(item) => <li>{item}</li>}</For>, "ul");
  const [a, b, c] = el.children;
  setItems(["c", "a", "b"]);
  tick();
  expect(texts(el)).toEqual(["c", "a", "b"]);
  expect([...el.children]).toEqual([c, a, b]);
});

test("For with keyed keeps the row of a changed item and updates item() in place", () => {
  type Row = { id: number; label: string };
  let builds = 0;
  const [items, setItems] = signal<Row[]>([
    { id: 1, label: "one" },
    { id: 2, label: "two" },
  ]);
  const { el } = mount(
    () => (
      <For each={items()} keyed={(row) => row.id}>
        {(row) => {
          builds++;
          return <li>{row().label}</li>;
        }}
      </For>
    ),
    "ul",
  );
  const [one, two] = el.children;
  setItems([
    { id: 2, label: "dos" },
    { id: 1, label: "uno" },
  ]);
  tick();
  expect(texts(el)).toEqual(["dos", "uno"]);
  expect([...el.children]).toEqual([two, one]);
  expect(builds).toBe(2);
});

test("For index() tracks the row position", () => {
  const [items, setItems] = signal(["a", "b", "c"]);
  const { el } = mount(
    () => (
      <For each={items()}>
        {(item, index) => (
          <li>
            {index()}:{item}
          </li>
        )}
      </For>
    ),
    "ul",
  );
  expect(texts(el)).toEqual(["0:a", "1:b", "2:c"]);
  setItems(["c", "a"]);
  tick();
  expect(texts(el)).toEqual(["0:c", "1:a"]);
});

test("For with keyed={false} keeps rows by position and updates item() in place", () => {
  let builds = 0;
  const [items, setItems] = signal(["a", "b", "c"]);
  const { el } = mount(
    () => (
      <For each={items()} keyed={false}>
        {(item, index) => {
          builds++;
          return (
            <li>
              {index}:{item()}
            </li>
          );
        }}
      </For>
    ),
    "ul",
  );
  const [first, second, third] = el.children;
  expect(texts(el)).toEqual(["0:a", "1:b", "2:c"]);
  setItems(["c", "a", "b"]);
  tick();
  expect(texts(el)).toEqual(["0:c", "1:a", "2:b"]);
  expect([...el.children]).toEqual([first, second, third]);
  expect(builds).toBe(3);
  setItems(["x"]);
  tick();
  expect(texts(el)).toEqual(["0:x"]);
  expect(builds).toBe(3);
  setItems(["x", "y"]);
  tick();
  expect(texts(el)).toEqual(["0:x", "1:y"]);
  expect(builds).toBe(4);
});

test("For disposes removed rows and shows the fallback while the list is empty", () => {
  const log: string[] = [];
  function Empty() {
    onCleanup(() => log.push("fallback"));
    return <li>empty</li>;
  }
  const [items, setItems] = signal<string[] | null>(["a", "b"]);
  const { el } = mount(
    () => (
      <For each={items()} fallback={<Empty />}>
        {(item) => {
          onCleanup(() => log.push(item));
          return <li>{item}</li>;
        }}
      </For>
    ),
    "ul",
  );
  setItems(["b"]);
  tick();
  expect(log).toEqual(["a"]);
  setItems([]);
  tick();
  expect(log).toEqual(["a", "b"]);
  expect(texts(el)).toEqual(["empty"]);
  setItems(["c"]);
  tick();
  expect(log).toEqual(["a", "b", "fallback"]);
  expect(texts(el)).toEqual(["c"]);
  setItems(null);
  tick();
  expect(texts(el)).toEqual(["empty"]);
});

test("For duplicate items map to distinct rows", () => {
  const [items, setItems] = signal(["x", "x", "y"]);
  const { el } = mount(() => <For each={items()}>{(item) => <li>{item}</li>}</For>, "ul");
  setItems(["y", "x", "x", "x"]);
  tick();
  expect(texts(el)).toEqual(["y", "x", "x", "x"]);
  expect(new Set(el.children).size).toBe(4);
});

test("random keyed updates keep DOM order and reuse the nodes of surviving rows", () => {
  let seed = 7;
  const random = (n: number) => (seed = (seed * 1103515245 + 12345) % 2 ** 31) % n;
  let nextId = 0;
  const [items, setItems] = signal<{ id: number }[]>([]);
  const { el } = mount(
    () => (
      <For each={items()} keyed={(item) => item.id}>
        {(item) => <li>{item().id}</li>}
      </For>
    ),
    "ul",
  );
  let nodes = new Map<number, Element>();
  for (let round = 0; round < 500; round++) {
    const list = items().slice();
    for (let k = random(4); k-- && list.length;) {
      list.splice(random(list.length), 1);
    }
    for (let k = random(4); k-- && list.length > 1;) {
      const [moved] = list.splice(random(list.length), 1);
      list.splice(random(list.length + 1), 0, moved!);
    }
    for (let k = random(4); k-- && list.length < 50;) {
      list.splice(random(list.length + 1), 0, { id: nextId++ });
    }
    setItems(list);
    tick();
    expect(texts(el)).toEqual(list.map((item) => String(item.id)));
    const children = [...el.children];
    list.forEach((item, i) => {
      const previous = nodes.get(item.id);
      if (previous !== undefined) {
        expect(children[i]).toBe(previous);
      }
    });
    nodes = new Map(list.map((item, i) => [item.id, children[i]!]));
  }
});

test("For with duplicate keys keeps every row and updates each in place", () => {
  const [items, setItems] = signal([
    { id: 1, label: "a" },
    { id: 1, label: "b" },
    { id: 2, label: "c" },
  ]);
  const { el } = mount(
    () => (
      <For each={items()} keyed={(item) => item.id}>
        {(item) => <li>{item().label}</li>}
      </For>
    ),
    "ul",
  );
  expect(texts(el)).toEqual(["a", "b", "c"]);
  setItems([
    { id: 2, label: "C" },
    { id: 1, label: "B" },
    { id: 1, label: "A" },
  ]);
  tick();
  expect(texts(el)).toEqual(["C", "B", "A"]);
  setItems([{ id: 1, label: "x" }]);
  tick();
  expect(texts(el)).toEqual(["x"]);
});

test("a selection change re-runs only the two rows whose comparison flips", () => {
  let runs = 0;
  const spy = () => {
    runs++;
  };
  const [rows, setRows] = signal([{ id: 1 }, { id: 2 }, { id: 3 }, { id: 4 }]);
  const [selected, setSelected] = signal(1);
  const { el } = mount(
    () => <For each={rows()}>{(row) => <li class={(spy(), selected() === row.id) ? "on" : ""}>{row.id}</li>}</For>,
    "ul",
  );
  const on = () => [...el.children].filter((li) => li.className === "on").map((li) => li.textContent);
  expect(runs).toBe(4);
  expect(on()).toEqual(["1"]);
  setSelected(3);
  tick();
  expect(runs).toBe(6);
  expect(on()).toEqual(["3"]);
  setSelected(4);
  tick();
  expect(runs).toBe(8);
  expect(on()).toEqual(["4"]);
  setSelected(0);
  tick();
  expect(runs).toBe(9);
  expect(on()).toEqual([]);
  setRows([{ id: 0 }, ...rows()]);
  tick();
  expect(runs).toBe(10);
  expect(on()).toEqual(["0"]);
});

test("rows reading a selector render like plain comparisons across selection and row changes", () => {
  const [rows, setRows] = signal([{ id: 1 }, { id: 2 }, { id: 3 }]);
  const [selected, setSelected] = signal(0);
  const { el: container } = mount(() => (
    <ul>
      <For each={rows()} keyed={(row) => row.id}>
        {(row) => (
          <li class={selected() === row().id ? "on" : ""} title={row().id !== selected() ? "off" : "on"}>
            {row().id}
            {selected() === row().id && <b>*</b>}
          </li>
        )}
      </For>
    </ul>
  ));
  const el = container.firstElementChild!;
  const row = (id: number, isOn: boolean) =>
    isOn ? `<li class="on" title="on">${id}<b>*</b></li>` : `<li class="" title="off">${id}</li>`;
  expect(el.innerHTML).toBe(row(1, false) + row(2, false) + row(3, false));
  setSelected(2);
  tick();
  expect(el.innerHTML).toBe(row(1, false) + row(2, true) + row(3, false));
  setSelected(3);
  tick();
  expect(el.innerHTML).toBe(row(1, false) + row(2, false) + row(3, true));
  setRows([{ id: 3 }, { id: 4 }, { id: 1 }]);
  tick();
  expect(el.innerHTML).toBe(row(3, true) + row(4, false) + row(1, false));
  setSelected(4);
  tick();
  expect(el.innerHTML).toBe(row(3, false) + row(4, true) + row(1, false));
  setRows([{ id: 5 }]);
  setSelected(5);
  tick();
  expect(el.innerHTML).toBe(row(5, true));
  setSelected(0);
  tick();
  expect(el.innerHTML).toBe(row(5, false));
});

test("keyed row writes reexecute on replacement without losing row identity", () => {
  type Row = { id: number; label: string };
  const makeRow = (id: number, label: string) => {
    const writes: number[] = [];
    const deleted: string[] = [];
    const target = Object.create({
      get id(): number { return id; },
      set id(value: number) { writes.push(value); },
    }) as Row;
    target.label = label;
    const row = new Proxy(target, {
      deleteProperty(object, key) {
        deleted.push(String(key));
        return Reflect.deleteProperty(object, key);
      },
    });
    return { row, writes, deleted };
  };
  const one = makeRow(1, "one");
  const two = makeRow(2, "two");
  const [rows, setRows] = signal([one.row, two.row]);
  const { el } = mount(
    () => (
      <For each={rows()} keyed={(row) => row.id}>
        {(row: (...args: number[]) => Row) => (
          <li data-label={row().label}>
            {(row().id = 5)}{row().id++}{delete (row() as Partial<Row>).id}{[row().id] = [7]}{row(1).id}
          </li>
        )}
      </For>
    ),
    "ul",
  );
  const [first, second] = [...el.children];
  expect(el.innerHTML).toBe('<li data-label="one">5171</li><li data-label="two">5272</li>');
  expect(one.writes).toEqual([5, 2, 7]);
  expect(two.writes).toEqual([5, 3, 7]);
  expect(one.deleted).toEqual(["id"]);
  expect(two.deleted).toEqual(["id"]);
  const dos = makeRow(2, "dos");
  const uno = makeRow(1, "uno");
  setRows([dos.row, uno.row]);
  tick();
  expect([...el.children]).toEqual([second, first]);
  expect(el.innerHTML).toBe('<li data-label="dos">5272</li><li data-label="uno">5171</li>');
  expect(dos.writes).toEqual([5, 3, 7]);
  expect(uno.writes).toEqual([5, 2, 7]);
  expect(dos.deleted).toEqual(["id"]);
  expect(uno.deleted).toEqual(["id"]);
});
