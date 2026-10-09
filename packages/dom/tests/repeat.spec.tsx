import { signal } from "@rezejs/signals";
import { cleanup, mount, tick } from "@rezejs/testing-library";
import { onCleanup, Repeat } from "reze-js";
import { afterEach, expect, test } from "vite-plus/test";

afterEach(cleanup);

test("Repeat renders one row per index and keeps the kept rows' nodes while count grows and shrinks", () => {
  const cleaned: number[] = [];
  const [count, setCount] = signal(2);
  const { el } = mount(() => (
    <ul>
      <Repeat count={count()}>
        {(index) => {
          onCleanup(() => cleaned.push(index));
          return <li>{index}</li>;
        }}
      </Repeat>
    </ul>
  ));
  const [first, second] = [...el.querySelectorAll("li")];
  expect(el.innerHTML).toBe("<ul><li>0</li><li>1</li></ul>");

  setCount(4);
  tick();
  expect(el.innerHTML).toBe("<ul><li>0</li><li>1</li><li>2</li><li>3</li></ul>");
  expect(el.querySelectorAll("li")[0]).toBe(first);
  expect(el.querySelectorAll("li")[1]).toBe(second);
  expect(cleaned).toEqual([]);

  setCount(1);
  tick();
  expect(el.innerHTML).toBe("<ul><li>0</li></ul>");
  expect(el.querySelector("li")).toBe(first);
  expect(cleaned.toSorted()).toEqual([1, 2, 3]);
});

test("Repeat shows its fallback while count is not a positive number, and truncates a fraction", () => {
  const [count, setCount] = signal(0);
  const { el } = mount(() => (
    <Repeat count={count()} fallback={<p>none</p>}>
      {(index) => <i>{index}</i>}
    </Repeat>
  ));
  expect(el.innerHTML).toBe("<p>none</p>");
  setCount(2.9);
  tick();
  expect(el.innerHTML).toBe("<i>0</i><i>1</i>");
  setCount(-3);
  tick();
  expect(el.innerHTML).toBe("<p>none</p>");
  setCount(Number.NaN);
  tick();
  expect(el.innerHTML).toBe("<p>none</p>");
});

test("Repeat with a constant count renders its rows once, indexed, and disposes them with the owner", () => {
  const cleaned: number[] = [];
  function Cell(props: { index: number }) {
    onCleanup(() => cleaned.push(props.index));
    return <li>{props.index}</li>;
  }
  const { el, dispose } = mount(() => (
    <ul>
      <Repeat count={3} fallback={<p>none</p>}>
        {(index) => <Cell index={index} />}
      </Repeat>
    </ul>
  ));
  expect(el.innerHTML).toBe("<ul><li>0</li><li>1</li><li>2</li></ul>");
  dispose();
  expect(cleaned.toSorted()).toEqual([0, 1, 2]);
});

test("Repeat with a constant count is not rebuilt by what its rows read, even inside a tracked child", () => {
  let builds = 0;
  const [label, setLabel] = signal("a");
  function Cell(props: { label: string }) {
    builds++;
    return <b>{props.label}</b>;
  }
  const view = () => <Repeat count={2}>{(index) => <Cell label={label() + index} />}</Repeat>;
  const { el } = mount(() => <div>{view()}</div>);
  expect(el.innerHTML).toBe("<div><b>a0</b><b>a1</b></div>");
  setLabel("b");
  tick();
  expect(el.innerHTML).toBe("<div><b>b0</b><b>b1</b></div>");
  expect(builds).toBe(2);
});

test("Repeat rows that take a ref keep working with a constant count", () => {
  const refs: Element[] = [];
  const { el } = mount(() => <Repeat count={2}>{() => <i ref={(node: Element) => refs.push(node)} />}</Repeat>);
  expect(el.innerHTML).toBe("<i></i><i></i>");
  expect(refs).toEqual([...el.children]);
});

test("Repeat calls row callbacks eagerly once per row", () => {
  const calls: string[] = [];
  const handler = (): (() => void) => {
    calls.push("tap");
    return () => {};
  };
  const { el } = mount(() => <Repeat count={2}>{() => <b onClick={handler()}>x</b>}</Repeat>);
  expect(el.innerHTML).toBe("<b>x</b><b>x</b>");
  expect(calls).toEqual(["tap", "tap"]);
  tick();
  expect(calls).toEqual(["tap", "tap"]);
});
