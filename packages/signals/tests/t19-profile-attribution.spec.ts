import { expect, test } from "vitest";

import { computed, effect, flush, root, signal } from "../src";
import { profileComponent, setProfileHook, startProfileSession, stopProfileSession, type ProfileEvent } from "../src/profile";

test("post-mount writes, reruns and disposal retain the component that created the node", () => {
  const events: ProfileEvent[] = [];
  setProfileHook({ event: (event) => void events.push(event) });
  startProfileSession();
  try {
    const instance = profileComponent("Card", "card.tsx#Card", 1, () =>
      root((dispose) => {
        const [value, setValue] = signal(0);
        const doubled = computed(() => value() * 2);
        effect(() => {
          doubled();
        });
        return { setValue, dispose };
      }),
    );
    instance.setValue(1);
    flush();
    instance.dispose();
    expect(stopProfileSession().components).toEqual([{ component: "Card", file: "card.tsx", mounts: 1, props: 1, reruns: 2, writes: 1 }]);
    const later = events.filter((event) => event.type !== "created");
    expect(later.some((event) => event.type === "write")).toBe(true);
    expect(later.some((event) => event.type === "rerun")).toBe(true);
    expect(later.some((event) => event.type === "dispose")).toBe(true);
    expect(later.every((event) => event.component === "Card" && event.file === "card.tsx")).toBe(true);
  } finally {
    stopProfileSession();
    setProfileHook(undefined);
  }
});

test("a write from another component remains attributed to its original creator", () => {
  startProfileSession();
  try {
    const setValue = profileComponent("Child", "child.tsx#Child", 0, () => {
      const [value, setValue] = signal(0);
      const doubled = computed(() => value() * 2);
      doubled();
      return (next: number) => {
        setValue(next);
        return doubled();
      };
    });
    profileComponent("Parent", "parent.tsx#Parent", 0, () => expect(setValue(2)).toBe(4));
    expect(stopProfileSession().components).toEqual([
      { component: "Child", file: "child.tsx", mounts: 1, props: 0, reruns: 1, writes: 1 },
      { component: "Parent", file: "parent.tsx", mounts: 1, props: 0, reruns: 0, writes: 0 },
    ]);
  } finally {
    stopProfileSession();
  }
});

test("nodes created outside a component do not borrow a later component scope", () => {
  const events: ProfileEvent[] = [];
  setProfileHook({ event: (event) => void events.push(event) });
  try {
    const [value, setValue] = signal(0);
    const doubled = computed(() => value() * 2);
    doubled();
    profileComponent("Unrelated", "other.tsx#Unrelated", 0, () => {
      setValue(1);
      expect(doubled()).toBe(2);
    });
    const later = events.filter((event) => event.type === "write" || event.type === "rerun");
    expect(later).toHaveLength(2);
    expect(later.every((event) => event.component === undefined && event.file === undefined)).toBe(true);
  } finally {
    setProfileHook(undefined);
  }
});
