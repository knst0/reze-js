import { cleanup, mount, settle, tick } from "@rezejs/testing-library";
import * as R from "reze-js";
import { $computed, $computed as derive, $signal, $signal as sig, effect, For, Show } from "reze-js";
import { computed, signal, signal as rawSignal } from "@rezejs/signals";
import { afterEach, expect, test } from "vitest";

afterEach(cleanup);

test("compiled signal assignments preserve expression results, precedence, and live DOM reads", () => {
  const results: number[] = [];
  const { el } = mount(() => {
    let count = $signal(8);
    return (
      <button
        onClick={() => {
          results.push((count -= 2), (count *= 2 + 1), (count /= 3), (count **= 2), (count %= 5));
          results.push((count <<= 2), (count >>= 1), (count >>>= 1), (count |= 2), (count &= 6), (count ^= 3));
          count++;
          ++count;
          count--;
          --count;
        }}
      >
        {count}
      </button>
    );
  });
  const button = el.querySelector("button")!;
  expect(button.textContent).toBe("8");
  button.click();
  tick();
  expect(results).toEqual([6, 18, 6, 36, 1, 4, 2, 1, 3, 2, 1]);
  expect(button.textContent).toBe("1");
  expect(el.firstChild).toBe(button);
});

test("compiled logical assignments skip unused operands and return the selected value", () => {
  const calls: string[] = [];
  const results: (number | undefined)[] = [];
  const produce = (name: string, value: number): number => {
    calls.push(name);
    return value;
  };
  const { el } = mount(() => {
    let value = $signal<number | undefined>(0);
    return (
      <button
        onClick={() => {
          results.push((value &&= produce("skipped-and", 1)));
          results.push((value ??= produce("skipped-nullish", 2)));
          results.push((value ||= produce("or", 3)));
          results.push((value ||= produce("skipped-or", 4)));
          results.push((value &&= produce("and", 5)));
          value = undefined;
          results.push((value ??= produce("nullish", 6)));
        }}
      >
        {value}
      </button>
    );
  });
  el.querySelector("button")!.click();
  tick();
  expect(calls).toEqual(["or", "and", "nullish"]);
  expect(results).toEqual([0, 0, 3, 3, 5, 6]);
  expect(el.textContent).toBe("6");
});

test("compiled function-valued signals retain closures, shorthand values, and alias bindings", () => {
  let invoke: () => { name: string; value: string };
  let calls = 0;
  const { el } = mount(() => {
    let name = sig("first");
    let handler = $signal<() => string>();
    const saved = () => {
      calls++;
      return name;
    };
    handler = saved;
    invoke = () => ({ name, value: handler!() });
    return <button onClick={() => (name = "second")}>{name}</button>;
  });
  expect(calls).toBe(0);
  expect(invoke!()).toEqual({ name: "first", value: "first" });
  el.querySelector("button")!.click();
  tick();
  expect(invoke!()).toEqual({ name: "second", value: "second" });
  expect(calls).toBe(2);
  expect(el.textContent).toBe("second");
});

test("compiled computed chains and namespace signals update effects and dispose subscriptions", () => {
  const observed: { count: number; area: number; label: string }[] = [];
  let increment: () => void;
  const { el, dispose } = mount(() => {
    let count = R.$signal(2);
    const area = $computed(count * count);
    const label = R.$computed(`area:${area}`);
    effect(() => {
      observed.push({ count, area, label });
    });
    increment = () => {
      count += 1;
    };
    return <output>{label}</output>;
  });
  expect(el.textContent).toBe("area:4");
  expect(observed).toEqual([{ count: 2, area: 4, label: "area:4" }]);
  increment!();
  tick();
  expect(el.textContent).toBe("area:9");
  expect(observed).toEqual([
    { count: 2, area: 4, label: "area:4" },
    { count: 3, area: 9, label: "area:9" },
  ]);
  dispose();
  increment!();
  tick();
  expect(observed).toEqual([
    { count: 2, area: 4, label: "area:4" },
    { count: 3, area: 9, label: "area:9" },
  ]);
});

test("compiled signal reads and writes drive classes, text, and neighboring bindings", () => {
  let readOther: () => number;
  let loopRuns = 0;
  const { el } = mount(() => {
    let count = $signal(0);
    let other = 1;
    other = 2;
    for (let i = $signal(0); i < 2; i++) {
      loopRuns++;
    }
    readOther = () => other;
    return (
      <button class={count > 1 ? "big" : ""} onClick={() => { count++; count += 2; }}>
        {count}
      </button>
    );
  });
  const button = el.querySelector("button")!;
  expect(button.textContent).toBe("0");
  expect(button.className).toBe("");
  expect(loopRuns).toBe(2);
  expect(readOther!()).toBe(2);
  button.click();
  tick();
  expect(button.textContent).toBe("3");
  expect(button.className).toBe("big");
  expect(el.firstChild).toBe(button);
});

test("compiled signal assignments store templates, shorthands, and branch results", () => {
  let run: (a: number, b: number, list: number[]) => unknown[];
  let pickCalls = 0;
  const { el } = mount(() => {
    let c = $signal<unknown>(0);
    const pick = (list: number[]): number => {
      pickCalls++;
      return list[0]!;
    };
    run = (a: number, b: number, list: number[]) => {
      const seen: unknown[] = [];
      c = 1;
      seen.push(c);
      c = `x${a}`;
      seen.push(c);
      c = { a };
      seen.push(c);
      c = [a];
      seen.push(c);
      c = a + b;
      seen.push(c);
      c = -a;
      seen.push(c);
      c = a ? 1 : 2;
      seen.push(c);
      c = a ? b : 2;
      seen.push(c);
      c = pick(list);
      seen.push(c);
      c = (c = 3);
      seen.push(c);
      return seen;
    };
    return <output>{String(c as number)}</output>;
  });
  expect(run!(5, 7, [9])).toEqual([1, "x5", { a: 5 }, [5], 12, -5, 1, 7, 9, 3]);
  expect(pickCalls).toBe(1);
  tick();
  expect(el.textContent).toBe("3");
  expect(run!(0, 7, [9])).toEqual([1, "x0", { a: 0 }, [0], 7, -0, 2, 2, 9, 3]);
  expect(pickCalls).toBe(2);
  tick();
  expect(el.textContent).toBe("3");
});

test("compiled signal updates run in loop and short-circuit positions in order", () => {
  let run: (ok: boolean) => number[];
  const { el } = mount(() => {
    let i = $signal(0);
    run = (ok: boolean) => {
      const trace: number[] = [];
      for (i = 0; i < 3; i++) {
        trace.push(i);
      }
      ok && i++;
      trace.push(i);
      ok ? i++ : i--;
      trace.push(i);
      i++, i--;
      trace.push(i);
      void i++;
      trace.push(i);
      return trace;
    };
    return <output>{i}</output>;
  });
  expect(run!(true)).toEqual([0, 1, 2, 4, 5, 5, 6]);
  tick();
  expect(el.textContent).toBe("6");
  expect(run!(false)).toEqual([0, 1, 2, 3, 2, 2, 3]);
  tick();
  expect(el.textContent).toBe("3");
});

test("compiled namespace, merged, and cross-module signal imports share one graph", () => {
  let setB!: (v: number) => void;
  let bump!: () => void;
  const { el } = mount(() => {
    let n = R.$signal(0);
    const [b, setBRaw] = rawSignal(1);
    setB = setBRaw;
    const sum = computed(() => n + b());
    bump = () => {
      n += 1;
    };
    return <output onClick={() => setB(2)}>{sum()}</output>;
  });
  expect(el.textContent).toBe("1");
  bump!();
  tick();
  expect(el.textContent).toBe("2");
  setB!(5);
  tick();
  expect(el.textContent).toBe("6");
});

test("compiled never-written signals render stable values while written ones stay live", () => {
  const { el } = mount(() => {
    let title = $signal("Reze");
    const fixed = $signal(1);
    let count = $signal(0);
    return <h1 onClick={() => (count += 1)}>{title}: {fixed} {count}</h1>;
  });
  const h1 = el.querySelector("h1")!;
  expect(h1.textContent).toBe("Reze: 1 0");
  h1.click();
  tick();
  expect(h1.textContent).toBe("Reze: 1 1");
  h1.click();
  tick();
  expect(h1.textContent).toBe("Reze: 1 2");
});

test("compiled signal closures and effects observe object, shorthand, and array reads", () => {
  const seen: unknown[][] = [];
  let inc!: () => void;
  let reset!: () => () => number;
  mount(() => {
    let count = $signal(0);
    effect(() => {
      seen.push([count, { count }, [count]]);
    });
    inc = () => {
      count++;
    };
    reset = () => {
      const log = () => count;
      count = 0;
      return log;
    };
    return <output>{count}</output>;
  });
  expect(seen).toEqual([[0, { count: 0 }, [0]]]);
  inc!();
  tick();
  expect(seen).toEqual([
    [0, { count: 0 }, [0]],
    [1, { count: 1 }, [1]],
  ]);
  const log = reset!();
  expect(log()).toBe(0);
  tick();
  expect(seen).toEqual([
    [0, { count: 0 }, [0]],
    [1, { count: 1 }, [1]],
    [0, { count: 0 }, [0]],
  ]);
});

test("compiled signals drive attributes, styles, inputs, and component props", () => {
  function Card(props: { size: number; children?: unknown }) {
    return <span data-size={props.size}>{props.children}</span>;
  }
  const { el } = mount(() => {
    let size = $signal(2);
    let text = $signal("a");
    return (
      <div>
        <div
          class={size > 1 ? "big" : ""}
          style={{ width: `${size * 10}px` }}
        >
          <input value={text} onInput={(e) => (text = e.currentTarget.value)} />
          <Card size={size}>
            {size} {text}
          </Card>
        </div>
        <button onClick={() => (size = 4)}>grow</button>
      </div>
    );
  });
  const box = el.firstChild!.firstChild as HTMLElement;
  const input = el.querySelector("input")!;
  const card = el.querySelector("span")!;
  expect(box.className).toBe("big");
  expect(box.style.width).toBe("20px");
  expect(input.value).toBe("a");
  expect(card.dataset.size).toBe("2");
  expect(card.textContent).toBe("2 a");
  input.value = "b";
  input.dispatchEvent(new Event("input", { bubbles: true }));
  tick();
  expect(input.value).toBe("b");
  expect(card.textContent).toBe("2 b");
  el.querySelector("button")!.click();
  tick();
  expect(box.style.width).toBe("40px");
  expect(card.dataset.size).toBe("4");
  expect(card.textContent).toBe("4 b");
});

test("compiled signals drive list rows and conditional branches", () => {
  const { el } = mount(() => {
    let rows = $signal([{ id: 1 }, { id: 2 }]);
    let selected = $signal(0);
    let open = $signal(false);
    return (
      <ul>
        <For each={rows}>
          {(row) => (
            <li class={selected === row.id ? "on" : ""} onClick={() => (selected = row.id)} />
          )}
        </For>
        <Show when={open} fallback={<i>closed</i>}>
          <p onClick={() => (open = false)}>open</p>
        </Show>
        <button onClick={() => { rows = [...rows, { id: rows.length + 1 }]; open = !open; }} />
      </ul>
    );
  });
  expect(el.querySelectorAll("li")).toHaveLength(2);
  expect(el.querySelector("i")?.textContent).toBe("closed");
  el.querySelectorAll("li")[0]!.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  tick();
  expect(el.querySelectorAll("li")[0]!.className).toBe("on");
  el.querySelector("button")!.click();
  tick();
  expect(el.querySelectorAll("li")).toHaveLength(3);
  expect(el.querySelector("p")?.textContent).toBe("open");
});

test("compiled typed and JSX-free signals expose live values through plain functions", () => {
  let inc!: () => void;
  let read!: () => number;
  const { el } = mount(() => {
    let count: number | undefined = $signal();
    let list: string[] = $signal<string[]>([]);
    let n = $signal(0);
    inc = () => {
      n += 1;
    };
    read = () => n;
    return <p onClick={() => (count = 1)}>{count ?? "empty"}{list.join(",")}</p>;
  });
  const p = el.querySelector("p")!;
  expect(p.textContent).toBe("empty");
  expect(read!()).toBe(0);
  inc!();
  expect(read!()).toBe(1);
  p.click();
  tick();
  expect(p.textContent).toBe("1");
});

test("compiled computed values update titles, handlers, shorthands, and derived objects", () => {
  const logged: unknown[] = [];
  const { el } = mount(() => {
    let count = $signal(1);
    const doubled = $computed(count * 2);
    const area = $computed(count * count, { name: "area" });
    const box = $computed({ count, area }, { name: `box${count}` });
    let half: number = $computed(count / 2);
    const label = $computed({ text: `x${doubled}` }, { name: "label" });
    return (
      <button
        title={String(doubled)}
        onClick={() => {
          logged.push({ doubled });
          count += doubled;
        }}
      >
        {label.text}: {doubled} {area} {box.area} {half}
      </button>
    );
  });
  const button = el.querySelector("button")!;
  expect(button.title).toBe("2");
  expect(button.textContent).toBe("x2: 2 1 1 0.5");
  button.click();
  tick();
  expect(logged).toEqual([{ doubled: 2 }]);
  expect(button.title).toBe("6");
  expect(button.textContent).toBe("x6: 6 9 9 1.5");
});

test("compiled computed alias, namespace, and merged imports derive from shared sources", () => {
  let setA!: (v: number) => void;
  const { el } = mount(() => {
    const [a, setARaw] = signal(1);
    setA = setARaw;
    const b = computed(() => a() + 1);
    const c = $computed(a() + b());
    const d = derive(a() + 1);
    const e = R.$computed(a() + 1);
    return <output>{c} {d} {e}</output>;
  });
  expect(el.textContent).toBe("3 2 2");
  setA!(2);
  tick();
  expect(el.textContent).toBe("5 3 3");
});

test("compiled computed values drive list selection and conditional branches", () => {
  const rows = [{ id: 1 }, { id: 2 }, { id: 4 }];
  const { el } = mount(() => {
    let picked = $signal(0);
    const selected = $computed(picked + 1);
    const big = $computed(picked > 3);
    return (
      <ul>
        <For each={rows}>
          {(row) => (
            <li class={selected === row.id ? "on" : ""} onClick={() => (picked = row.id)} />
          )}
        </For>
        <Show when={big}>
          <p>big</p>
        </Show>
      </ul>
    );
  });
  const items = el.querySelectorAll("li");
  expect(items[0]!.className).toBe("on");
  expect(el.querySelector("p")).toBeNull();
  items[2]!.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  tick();
  expect(el.querySelector("p")?.textContent).toBe("big");
});

test("compiled signals and computed values survive async component awaits", async () => {
  const gate = Promise.withResolvers<string>();
  async function Card(props: { id: number }) {
    let n = $signal(1);
    const twice = $computed(n * 2);
    const user = await gate.promise;
    const title = $computed(`${user}${props.id}:${n}`);
    return <p title={title} onClick={() => (n += 1)}>{user}{twice}</p>;
  }
  const { el } = mount(() => <Card id={1} />);
  expect(el.innerHTML).toBe("");
  gate.resolve("a");
  await settle();
  tick();
  const p = el.querySelector("p")!;
  expect(p.title).toBe("a1:1");
  expect(p.textContent).toBe("a2");
  p.click();
  tick();
  expect(p.title).toBe("a1:2");
  expect(p.textContent).toBe("a4");
});

test("compiled signals with equals:false notify on same values while defaults do not", () => {
  const liveRuns: string[] = [];
  const plainRuns: string[] = [];
  mount(() => {
    let name = $signal("a", { equals: false });
    let plain = $signal("a");
    effect(() => {
      liveRuns.push(name);
    });
    effect(() => {
      plainRuns.push(plain);
    });
    name = "a";
    plain = "a";
    return <output>{name}{plain}</output>;
  });
  tick();
  expect(liveRuns).toEqual(["a", "a"]);
  expect(plainRuns).toEqual(["a"]);
});

