import { action as rawAction } from "@rezejs/signals";
import { cleanup, mount, settle, tick } from "@rezejs/testing-library";
import * as R from "reze-js";
import { action, action as act, signal, effect, For, Show, store } from "reze-js";
import { afterEach, expect, test } from "vite-plus/test";

afterEach(cleanup);

test("a compiled `action` shows its store writes before the save settles, rolls back a failed one, and keeps parallel toggles apart", async () => {
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
  const toggle = action(async (todo: { done: boolean }) => {
    todo.done = !todo.done;
    await save();
  });
  const { el } = mount(() => (
    <ul>
      <For each={todos}>
        {(todo) => (
          <li class={{ done: todo.done }}>
            <input type="checkbox" checked={todo.done} onChange={() => toggle(todo).catch(() => {})} />
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

test("a compiled action awaits nested operands in order and returns the final value", async () => {
  const order: string[] = [];
  const g = async (): Promise<number> => {
    order.push("g");
    return 1;
  };
  const h = async (): Promise<number> => {
    order.push("h");
    return 2;
  };
  const f = async (x: number, y: number): Promise<number> => {
    order.push("f");
    return x + y;
  };
  const box = store({ text: "pending" });
  const load = action(async () => {
    return await f(await g(), await await h());
  });
  const { el } = mount(() => {
    const fire = (): void => {
      load()
        .then((v) => {
          box.text = `got ${v}`;
        })
        .catch(() => {
          box.text = "failed";
        });
    };
    return <button onClick={fire}>{box.text}</button>;
  });
  el.querySelector("button")!.click();
  await settle();
  tick();
  expect(order).toEqual(["g", "h", "f"]);
  expect(el.textContent).toBe("got 3");
});

test("a compiled action supports expression bodies, bare parameters, and destructured defaults", async () => {
  const seen: unknown[][] = [];
  const put = async (...args: unknown[]): Promise<string> => {
    seen.push(args);
    return args.join("|");
  };
  const save = action(async (x: number) => await put(x));
  const pick = action((list: number[]) => ({ first: list[0] }));
  const withDefaults = action(async ({ id }: { id: number }, [first] = [0], ...rest: number[]) => await put(id, first, rest));
  expect(await save(7)).toBe("7");
  expect(await pick([9])).toEqual({ first: 9 });
  expect(await withDefaults({ id: 5 }, [10], 20, 30)).toBe("5|10|20,30");
  expect(seen).toEqual([[7], [5, 10, [20, 30]]]);
});

test("a compiled action routes rejections through catch and finally with balanced cleanup", async () => {
  const snapshots: string[] = [];
  const gates: PromiseWithResolvers<number>[] = [];
  const put = (): Promise<number> => {
    const gate = Promise.withResolvers<number>();
    gates.push(gate);
    return gate.promise;
  };
  const t = store({ value: 0, error: "", busy: false });
  const save = action(async (s: typeof t) => {
    s.busy = true;
    try {
      s.value = await put();
    } catch {
      s.error = "failed";
    } finally {
      s.busy = false;
    }
  });
  const { el } = mount(() => {
    effect(() => {
      snapshots.push(`${t.value}|${t.error}|${t.busy}`);
    });
    return <output>{`${t.value}|${t.error}|${t.busy}|${save.pending}`}</output>;
  });
  expect(el.textContent).toBe("0||false|0");
  const failed = save(t).catch(() => {});
  tick();
  expect(el.textContent).toBe("0||true|1");
  gates[0]!.reject(new Error("offline"));
  await failed;
  await settle();
  tick();
  expect(t.value).toBe(0);
  expect(t.error).toBe("failed");
  expect(t.busy).toBe(false);
  expect(save.pending).toBe(0);
  expect(save.error).toBeUndefined();
  expect(snapshots[0]).toBe("0||false");
  expect(snapshots.at(-1)).toBe("0|failed|false");
  const ok = save(t);
  tick();
  gates[1]!.resolve(42);
  await ok;
  tick();
  expect(el.textContent).toBe("42|failed|false|0");
  expect(save.pending).toBe(0);
});

test("a compiled action keeps synchronous nested writes in its run next to signal writes", async () => {
  const gate = Promise.withResolvers<number>();
  let fire!: () => Promise<number>;
  let pending!: () => number;
  let seen!: () => boolean;
  const { el } = mount(() => {
    let saving = signal(0);
    const items = store([{ seen: false }]);
    const save = action(async (list: typeof items) => {
      saving += 1;
      list.forEach((item) => {
        item.seen = true;
      });
      const later = async (): Promise<number> => gate.promise;
      const n = await later();
      saving -= 1;
      return n;
    });
    fire = () => save(items);
    pending = () => save.pending;
    seen = () => items[0]!.seen;
    return <output>{saving}</output>;
  });
  expect(el.textContent).toBe("0");
  const result = fire();
  tick();
  expect(el.textContent).toBe("1");
  expect(seen()).toBe(true);
  expect(pending()).toBe(1);
  gate.resolve(5);
  expect(await result).toBe(5);
  tick();
  expect(el.textContent).toBe("0");
  expect(seen()).toBe(true);
  expect(pending()).toBe(0);
});

test("a compiled action works through namespace and alias imports without clobbering locals", async () => {
  const _a$ = 1;
  const a = R.action(async () => {
    await Promise.resolve();
    return _a$;
  });
  const b = act((value: number) => value + _a$);
  const passthrough = rawAction(() => _a$ + 5);
  await expect(a()).resolves.toBe(1);
  await expect(b(4)).resolves.toBe(5);
  await expect(passthrough()).resolves.toBe(6);
});

test("a compiled action inside an async component captures awaited state", async () => {
  const fetchGate = Promise.withResolvers<{ name: string }>();
  const saveGate = Promise.withResolvers<void>();
  const fetchUser = (): Promise<{ name: string }> => fetchGate.promise;
  const saveUser = (): Promise<void> => saveGate.promise;
  async function Card() {
    const data = await fetchUser();
    const user = store(data);
    const rename = action(async (name: string) => {
      user.name = name;
      await saveUser();
    });
    return <button onClick={() => rename("x").catch(() => {})}>{user.name}</button>;
  }
  const { el } = mount(() => <Card />);
  expect(el.innerHTML).toBe("");
  fetchGate.resolve({ name: "ann" });
  await settle();
  tick();
  const button = el.querySelector("button")!;
  expect(button.textContent).toBe("ann");
  button.click();
  tick();
  expect(button.textContent).toBe("x");
  saveGate.resolve();
  await settle();
  tick();
  expect(button.textContent).toBe("x");
});

test("a compiled synchronous action writes at once and settles without awaiting", async () => {
  const state = store({ n: 0 });
  const bump = action((s: { n: number }) => {
    s.n += 1;
  });
  const result = bump(state);
  expect(state.n).toBe(1);
  expect(bump.pending).toBe(0);
  await result;
});
