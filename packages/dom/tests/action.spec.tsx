import { $action, For, Show, store } from "reze-js";
import { afterEach, expect, test } from "vitest";

import { cleanup, mount, tick } from "../../../testing/dom";

afterEach(cleanup);

function settle(): Promise<void> {
  const { promise, resolve } = Promise.withResolvers<void>();
  setTimeout(resolve, 0);
  return promise;
}

test("a compiled `$action` shows its store writes before the save settles, rolls back a failed one, and keeps parallel toggles apart", async () => {
  const todos = store([
    { id: 1, done: false },
    { id: 2, done: false },
  ]);
  const saves: PromiseWithResolvers<void>[] = [];
  const save = (): Promise<void> => {
    const pending = Promise.withResolvers<void>();
    saves.push(pending);
    return pending.promise;
  };
  const toggle = $action(async (todo: { done: boolean }) => {
    todo.done = !todo.done;
    await save();
  });
  const { el } = mount(() => (
    <ul>
      <For each={todos}>
        {(todo) => (
          <li class={{ done: todo().done }}>
            <input type="checkbox" checked={todo().done} onChange={() => toggle(todo()).catch(() => {})} />
          </li>
        )}
      </For>
      <Show when={toggle.pending > 0}>
        <p>Saving</p>
      </Show>
    </ul>
  ));
  const rows = [...el.querySelectorAll("li")];
  for (const row of rows) row.querySelector("input")!.click();
  tick();
  expect(rows.map((row) => row.className)).toEqual(["done", "done"]);
  expect(el.querySelector("p")?.textContent).toBe("Saving");

  saves[0]!.reject(new Error("offline"));
  saves[1]!.resolve();
  await settle();
  tick();
  expect(rows.map((row) => row.className)).toEqual(["", "done"]);
  expect(rows.map((row) => row.querySelector("input")!.checked)).toEqual([false, true]);
  expect(todos.map((todo) => todo.done)).toEqual([false, true]);
  expect(el.querySelector("p")).toBeNull();
});
