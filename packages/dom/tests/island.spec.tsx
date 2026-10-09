import { cleanup, fire, mount, settle, tick } from "@rezejs/testing-library";
import { island, type JSX } from "reze-js";
import { afterEach, expect, test, vi } from "vite-plus/test";

afterEach(() => {
  try {
    cleanup();
  } finally {
    vi.unstubAllGlobals();
  }
});

function mockIdle(): () => void {
  const pending = new Map<number, () => void>();
  let nextId = 0;
  vi.stubGlobal("requestIdleCallback", (callback: () => void) => {
    const id = ++nextId;
    pending.set(id, callback);
    return id;
  });
  vi.stubGlobal("cancelIdleCallback", (id: number) => pending.delete(id));
  return () => {
    for (const [id, callback] of pending) {
      pending.delete(id);
      callback();
    }
  };
}

function Greeting(props: { name: string }) {
  return <b>hi {props.name}</b>;
}

test("an eager island with a local component renders at once, without a fallback flash", () => {
  let calls = 0;
  const { el } = mount(() =>
    island(
      "eager",
      () => {
        calls++;
        return Greeting;
      },
      { name: "ann" },
      () => <i>wait</i>,
    ),
  );
  expect(el.innerHTML).toBe("<b>hi ann</b>");
  expect(calls).toBe(1);
});

test("an eager island with a split chunk shows its fallback until the component arrives", async () => {
  let resolve!: (component: typeof Greeting) => void;
  const gate = new Promise<typeof Greeting>((done) => {
    resolve = done;
  });
  const { el } = mount(() =>
    island(
      "eager",
      () => gate,
      { name: "ann" },
      () => <i>wait</i>,
    ),
  );
  expect(el.innerHTML).toBe("<i>wait</i>");

  resolve(Greeting);
  await settle();
  expect(el.innerHTML).toBe("<b>hi ann</b>");
});

test("an idle island does not load until the browser is idle", async () => {
  const idle = mockIdle();
  let calls = 0;
  const { el } = mount(() =>
    island(
      "idle",
      () => {
        calls++;
        return Greeting;
      },
      { name: "ann" },
      () => <i>wait</i>,
    ),
  );
  await settle();
  expect(calls).toBe(0);
  expect(el.innerHTML).toBe("<i>wait</i>");

  idle();
  await settle();
  expect(el.innerHTML).toBe("<b>hi ann</b>");
  expect(calls).toBe(1);
});

test("a visible island wraps its fallback in a shell and loads when it scrolls into view", async () => {
  type Callback = (entries: { isIntersecting: boolean }[]) => void;
  let callback: Callback = () => {};
  let observed: Element[] = [];
  let disconnects = 0;
  vi.stubGlobal(
    "IntersectionObserver",
    class {
      constructor(cb: Callback) {
        callback = cb;
      }
      observe = (target: Element): void => {
        observed.push(target);
      };
      disconnect = (): void => {
        disconnects++;
      };
    },
  );
  let calls = 0;
  const { el } = mount(() =>
    island(
      "visible",
      () => {
        calls++;
        return Greeting;
      },
      { name: "ann" },
      () => <i>wait</i>,
    ),
  );
  await settle();
  expect(calls).toBe(0);
  const shell = el.querySelector("span[data-island='visible']")!;
  expect(shell.innerHTML).toBe("<i>wait</i>");
  expect(observed).toEqual([shell]);

  callback([{ isIntersecting: true }]);
  await settle();
  expect(el.innerHTML).toBe("<b>hi ann</b>");
  expect(calls).toBe(1);
  expect(disconnects).toBeGreaterThanOrEqual(1);
});

test("a visible island without a fallback renders a sized shell", () => {
  vi.stubGlobal(
    "IntersectionObserver",
    class {
      observe = (): void => {};
      disconnect = (): void => {};
    },
  );
  const { el } = mount(() => island("visible", () => Greeting, { name: "ann" }));
  expect(el.innerHTML).toBe('<span data-island="visible" style="display:block;min-width:1px;min-height:1px"></span>');
});

test("a media island loads at once when its query already matches", async () => {
  const listeners = new Set<() => void>();
  vi.stubGlobal("matchMedia", () => ({
    matches: true,
    addEventListener: (_type: string, callback: () => void): void => {
      listeners.add(callback);
    },
    removeEventListener: (_type: string, callback: () => void): void => {
      listeners.delete(callback);
    },
  }));
  let calls = 0;
  const { el } = mount(() =>
    island(
      "media",
      () => {
        calls++;
        return Greeting;
      },
      { name: "ann" },
      undefined,
      { media: "(max-width: 40rem)" },
    ),
  );
  await settle();
  expect(el.innerHTML).toBe("<b>hi ann</b>");
  expect(calls).toBe(1);
  expect(listeners.size).toBe(0);
});

test("a media island waits for its query to match", async () => {
  const listeners = new Set<() => void>();
  let matches = false;
  vi.stubGlobal("matchMedia", (query: string) => {
    expect(query).toBe("(max-width: 40rem)");
    return {
      get matches() {
        return matches;
      },
      addEventListener: (_type: string, callback: () => void): void => {
        listeners.add(callback);
      },
      removeEventListener: (_type: string, callback: () => void): void => {
        listeners.delete(callback);
      },
    };
  });
  let calls = 0;
  const { el } = mount(() =>
    island(
      "media",
      () => {
        calls++;
        return Greeting;
      },
      { name: "ann" },
      () => <i>wait</i>,
      { media: "(max-width: 40rem)" },
    ),
  );
  await settle();
  expect(calls).toBe(0);
  expect(el.innerHTML).toBe("<i>wait</i>");

  matches = true;
  for (const listener of listeners) {
    listener();
  }
  await settle();
  expect(el.innerHTML).toBe("<b>hi ann</b>");
  expect(calls).toBe(1);
  expect(listeners.size).toBe(0);
});

test("an interaction island loads on the first pointer event inside its shell", async () => {
  let calls = 0;
  const { el } = mount(() =>
    island(
      "interaction",
      () => {
        calls++;
        return Greeting;
      },
      { name: "ann" },
      () => <i>wait</i>,
    ),
  );
  await settle();
  expect(calls).toBe(0);

  fire(el.querySelector("span[data-island='interaction']")!, "pointerdown");
  await settle();
  expect(el.innerHTML).toBe("<b>hi ann</b>");
  expect(calls).toBe(1);
});

test("a failed island load goes to the failure view of the async component around it", async () => {
  async function Host() {
    await Promise.resolve();
    return island("eager", () => Promise.reject<(props: { name: string }) => JSX.Element>(new Error("offline")), { name: "ann" });
  }
  Host.failure = (error: unknown) => <em>{(error as Error).message}</em>;
  const { el } = mount(() => <Host />);
  await settle();
  expect(el.innerHTML).toBe("<em>offline</em>");
});

test("the island attribute compiles to a deferred island", async () => {
  const idle = mockIdle();
  const { el } = mount(() => <Greeting island="idle" name="ann" />);
  expect(el.innerHTML).toBe("");

  idle();
  await settle();
  tick();
  expect(el.innerHTML).toBe("<b>hi ann</b>");
});

test("the island attribute forwards children to the loaded component", async () => {
  const idle = mockIdle();
  function Panel(props: { title: string; children?: JSX.Element }) {
    return (
      <section>
        <h1>{props.title}</h1>
        {props.children}
      </section>
    );
  }
  const { el } = mount(() => (
    <Panel island="idle" title="t">
      body <b>bold</b>
    </Panel>
  ));
  expect(el.innerHTML).toBe("");

  idle();
  await settle();
  tick();
  expect(el.innerHTML).toBe("<section><h1>t</h1>body <b>bold</b></section>");
});
