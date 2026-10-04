import { cleanup, mount, tick } from "@rezejs/testing-library";
import * as R from "reze-js";
import { $computed, $signal, $signal as sig, effect } from "reze-js";
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
