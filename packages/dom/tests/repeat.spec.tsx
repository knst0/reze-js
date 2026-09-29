import { onCleanup, Repeat, signal } from "reze-js";
import { afterEach, expect, test } from "vitest";

import { cleanup, mount, tick } from "../../../testing/dom";

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
