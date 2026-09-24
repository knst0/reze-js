import { flushSync, onCleanup, signal } from "@rezejs/signals";
import { cleanup, mount } from "@rezejs/test-utils";
import { afterEach, expect, test } from "vitest";

import { Dynamic, Match, Portal, Show, Switch } from "../src/flow";

afterEach(cleanup);

test("Show keeps its branch while `when` stays truthy and passes the value as a getter", () => {
  let builds = 0;
  const [user, setUser] = signal<{ name: string } | null>({ name: "a" });
  const { el } = mount(() => (
    <Show when={user()} fallback={<i>none</i>}>
      {(u: () => { name: string }) => {
        builds++;
        return <b>{u().name}</b>;
      }}
    </Show>
  ));
  const b = el.firstChild;
  setUser({ name: "b" });
  flushSync();
  expect(el.innerHTML).toBe("<b>b</b>");
  expect(el.firstChild).toBe(b);
  expect(builds).toBe(1);
  setUser(null);
  flushSync();
  expect(el.innerHTML).toBe("<i>none</i>");
  setUser({ name: "c" });
  flushSync();
  expect(el.innerHTML).toBe("<b>c</b>");
  expect(builds).toBe(2);
});

test("Show disposes the branch it switches away from", () => {
  const log: string[] = [];
  function Child() {
    onCleanup(() => log.push("child"));
    return <b />;
  }
  const [on, setOn] = signal(true);
  mount(() => (
    <Show when={on()}>
      <Child />
    </Show>
  ));
  setOn(false);
  flushSync();
  expect(log).toEqual(["child"]);
});

test("Switch renders the first truthy Match and rebuilds only when the choice changes", () => {
  let builds = 0;
  const [n, setN] = signal(1);
  const { el } = mount(() => (
    <Switch fallback={<i>zero</i>}>
      <Match when={n() > 10}>
        <b>big</b>
      </Match>
      <Match when={n() > 0}>
        {(() => {
          builds++;
          return <b>small</b>;
        })()}
      </Match>
    </Switch>
  ));
  expect(el.innerHTML).toBe("<b>small</b>");
  setN(2);
  flushSync();
  expect(builds).toBe(1);
  setN(20);
  flushSync();
  expect(el.innerHTML).toBe("<b>big</b>");
  setN(0);
  flushSync();
  expect(el.innerHTML).toBe("<i>zero</i>");
});

test("Dynamic renders a tag name or a component with the remaining props", () => {
  const Comp = (props: { title?: string }) => <em>{props.title}</em>;
  const [c, setC] = signal<string | typeof Comp>("section");
  const [title, setTitle] = signal("t");
  const { el } = mount(() => <Dynamic component={c()} title={title()} />);
  expect(el.innerHTML).toBe('<section title="t"></section>');
  setTitle("u");
  flushSync();
  expect(el.innerHTML).toBe('<section title="u"></section>');
  setC(() => Comp);
  flushSync();
  expect(el.innerHTML).toBe("<em>u</em>");
});

test("Portal renders into mount and is removed with its owner", () => {
  const target = document.createElement("aside");
  document.body.appendChild(target);
  const [on, setOn] = signal(true);
  const [text, setText] = signal("a");
  const { el } = mount(() => (
    <div>
      <Show when={on()}>
        <Portal mount={target}>
          <p>{text()}</p>
        </Portal>
      </Show>
    </div>
  ));
  expect(el.innerHTML).toBe("<div></div>");
  expect(target.innerHTML).toBe("<p>a</p>");
  setText("b");
  flushSync();
  expect(target.innerHTML).toBe("<p>b</p>");
  setOn(false);
  flushSync();
  expect(target.innerHTML).toBe("");
});
