import { Bench } from "tinybench";

import { finishBench, withGraph } from "../../../benches/kit/micro";
import { flush, root, signal } from "../src/index";
import { fixedRenderEffect, renderEffect } from "../src/render";

const BINDINGS = 1000;

const sink = new Float64Array(BINDINGS);

const bench = new Bench();

{
  let setters: ((value: number) => number)[] = [];
  let next = 0;
  const hooks = withGraph(root, () => {
    setters = [];
    for (let i = 0; i < BINDINGS; i++) {
      const [get, set] = signal(0);
      setters.push(set);
      renderEffect(() => {
        sink[i] = get();
      });
    }
  });
  bench.add(
    "rerun: 1000 bindings x 1 dep, write all + flush / dynamic",
    () => {
      next += 1;
      for (let i = 0; i < BINDINGS; i++) setters[i]!(next);
      flush();
    },
    hooks,
  );
}

{
  let setters: ((value: number) => number)[] = [];
  let next = 0;
  const hooks = withGraph(root, () => {
    setters = [];
    for (let i = 0; i < BINDINGS; i++) {
      const [get, set] = signal(0);
      setters.push(set);
      fixedRenderEffect(() => {
        sink[i] = get();
      });
    }
  });
  bench.add(
    "rerun: 1000 bindings x 1 dep, write all + flush / fixed",
    () => {
      next += 1;
      for (let i = 0; i < BINDINGS; i++) setters[i]!(next);
      flush();
    },
    hooks,
  );
}

{
  let setters: ((value: number) => number)[] = [];
  let next = 0;
  const hooks = withGraph(root, () => {
    setters = [];
    for (let i = 0; i < BINDINGS; i++) {
      const [a, setA] = signal(0);
      const [b] = signal(1);
      const [c] = signal(2);
      setters.push(setA);
      renderEffect(() => {
        sink[i] = a() + b() + c();
      });
    }
  });
  bench.add(
    "rerun: 1000 bindings x 3 deps, write 1 each + flush / dynamic",
    () => {
      next += 1;
      for (let i = 0; i < BINDINGS; i++) setters[i]!(next);
      flush();
    },
    hooks,
  );
}

{
  let setters: ((value: number) => number)[] = [];
  let next = 0;
  const hooks = withGraph(root, () => {
    setters = [];
    for (let i = 0; i < BINDINGS; i++) {
      const [a, setA] = signal(0);
      const [b] = signal(1);
      const [c] = signal(2);
      setters.push(setA);
      fixedRenderEffect(() => {
        sink[i] = a() + b() + c();
      });
    }
  });
  bench.add(
    "rerun: 1000 bindings x 3 deps, write 1 each + flush / fixed",
    () => {
      next += 1;
      for (let i = 0; i < BINDINGS; i++) setters[i]!(next);
      flush();
    },
    hooks,
  );
}

bench.add("create: 1000 bindings x 1 dep / dynamic", () => {
  root((dispose) => {
    for (let i = 0; i < BINDINGS; i++) {
      const [get] = signal(i);
      renderEffect(() => {
        sink[i] = get();
      });
    }
    dispose();
  });
});

bench.add("create: 1000 bindings x 1 dep / fixed", () => {
  root((dispose) => {
    for (let i = 0; i < BINDINGS; i++) {
      const [get] = signal(i);
      fixedRenderEffect(() => {
        sink[i] = get();
      });
    }
    dispose();
  });
});

await finishBench(bench);
