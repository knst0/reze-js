import { expect, test } from "vitest";

import { computed, effect, signal } from "../src";
import { profileComponent, setProfileHook, startProfileSession, stopProfileSession, type ProfileEvent } from "../src/profile";

test("a session counts mounts, props, reruns and writes per component into a serializable tree", () => {
  startProfileSession();
  profileComponent("Card", "app.tsx#Card", 2, () => {
    const [n, setN] = signal(0);
    const doubled = computed(() => n() * 2);
    expect(doubled()).toBe(0);
    setN(1);
    expect(doubled()).toBe(2);
  });
  const tree = stopProfileSession();
  expect(tree).toEqual({
    v: 1,
    components: [{ component: "Card", file: "app.tsx", mounts: 1, props: 2, reruns: 1, writes: 1 }],
  });
  expect(JSON.parse(JSON.stringify(tree))).toEqual(tree);
});

test("first evaluations are not reruns", () => {
  startProfileSession();
  profileComponent("Row", undefined, 0, () => {
    const [n] = signal(0);
    let seen = 0;
    effect(() => {
      n();
      seen += 1;
    });
    expect(seen).toBe(1);
  });
  expect(stopProfileSession().components).toEqual([{ component: "Row", file: "", mounts: 1, props: 0, reruns: 0, writes: 0 }]);
});

test("events carry the innermost component and file, restoring the outer scope on exit", () => {
  const events: ProfileEvent[] = [];
  setProfileHook({ event: (event) => void events.push(event) });
  startProfileSession();
  profileComponent("App", "a.tsx#App", 1, () => {
    const [n] = signal(0, { name: "n" });
    profileComponent("Row", "a.tsx#Row", 0, () => {
      const [m] = signal(1, { name: "m" });
      expect(m()).toBe(1);
    });
    expect(n()).toBe(0);
  });
  stopProfileSession();
  setProfileHook(undefined);
  expect(events.filter((event) => event.type === "created").map((event) => [event.component, event.file, event.name])).toEqual([
    ["App", "a.tsx", "n"],
    ["Row", "a.tsx", "m"],
  ]);
});

test("stopping a session restores the previously installed hook", () => {
  const seen: ProfileEvent[] = [];
  setProfileHook({ event: (event) => void seen.push(event) });
  startProfileSession();
  stopProfileSession();
  signal(0);
  expect(seen.some((event) => event.type === "created" && event.kind === "signal")).toBe(true);
  setProfileHook(undefined);
});
