import { signal } from "@rezejs/signals";
import { cleanup, mount, settle, tick } from "@rezejs/testing-library";
import { catchError, effect, flush, onCleanup, root, Show } from "reze-js";
import { asyncComputed, Loading } from "reze-js/internal/async";
import { asyncComponent } from "reze-js/internal/runtime";
import { afterEach, expect, test } from "vitest";

afterEach(cleanup);

function requestsOf(): { requests: Record<number, PromiseWithResolvers<string>>; load: (n: number) => Promise<string> } {
  const requests: Record<number, PromiseWithResolvers<string>> = {};
  const load = (n: number): Promise<string> => {
    requests[n] = Promise.withResolvers<string>();
    return requests[n].promise;
  };
  return { requests, load };
}

test("an async component renders once its await settles, and a reload updates its values in place, keeping the nodes and state its body built", async () => {
  const [id, setId] = signal(1);
  const { requests, load } = requestsOf();
  async function User(props: { id: number }) {
    const name = await load(props.id);
    const [likes, setLikes] = signal(0);
    return (
      <button onClick={() => setLikes((n) => n + 1)}>
        {name}:{likes()}
      </button>
    );
  }
  const { el } = mount(() => <User id={id()} />);
  expect(el.innerHTML).toBe("");

  requests[1].resolve("a");
  await settle();
  expect(el.innerHTML).toBe("<button>a:0</button>");

  el.querySelector("button")!.click();
  tick();
  expect(el.innerHTML).toBe("<button>a:1</button>");
  const button = el.querySelector("button");

  setId(2);
  tick();
  expect(el.innerHTML).toBe("<button>a:1</button>");

  requests[2].resolve("b");
  await settle();
  expect(el.innerHTML).toBe("<button>b:1</button>");
  expect(el.querySelector("button")).toBe(button);
});

test("a source read by a later await reloads the component", async () => {
  const [suffix, setSuffix] = signal("x");
  const echoed: string[] = [];
  async function echo(value: string): Promise<string> {
    echoed.push(value);
    return value;
  }
  async function Label(props: { prefix: string }) {
    const base = await echo(props.prefix);
    const full = await echo(base + suffix());
    return <p>{full}</p>;
  }
  const { el } = mount(() => <Label prefix="a" />);
  await settle();
  expect(el.innerHTML).toBe("<p>ax</p>");

  setSuffix("y");
  tick();
  await settle();
  expect(el.innerHTML).toBe("<p>ay</p>");
  expect(echoed).toEqual(["a", "ax", "a", "ay"]);
});

test("the body of an async component never re-runs for the sources it reads", async () => {
  const [id, setId] = signal(1);
  const [label, setLabel] = signal("a");
  const bodies: string[] = [];
  const seen: unknown[] = [];
  root(() => {
    const child = asyncComponent(
      async (c) => [c.get(id) * 10],
      (values) => {
        bodies.push(`${values()[0]}${label()}`);
        return () => values()[0];
      },
    );
    effect(() => {
      seen.push(child()?.());
    });
  });
  await settle();
  setLabel("b");
  flush();
  setId(2);
  flush();
  await settle();

  expect(seen).toEqual([undefined, 10, 20]);
  expect(bodies).toEqual(["10a"]);
});

test("a reload keeps what the body created until the component is disposed", async () => {
  const [id, setId] = signal(1);
  const cleaned: string[] = [];
  let dispose!: () => void;
  root((d) => {
    dispose = d;
    const child = asyncComponent(
      async (c) => [c.get(id)],
      (values) => {
        onCleanup(() => cleaned.push("body"));
        return values()[0];
      },
    );
    effect(() => {
      child();
    });
  });
  await settle();
  setId(2);
  flush();
  await settle();

  expect(cleaned).toEqual([]);
  dispose();
  expect(cleaned).toEqual(["body"]);
});

test("a rejected load reaches catchError and the previous content stays", async () => {
  const [id, setId] = signal(1);
  const errors: unknown[] = [];
  let read!: () => number | undefined;
  root(() => {
    catchError(
      () => {
        const child = asyncComponent(
          async (c) => {
            const current = c.get(id);
            if (current === 2) throw new Error("boom");
            return [current];
          },
          (values) => values()[0],
        );
        effect(() => {
          child();
        });
        read = child;
      },
      (error) => errors.push(error),
    );
  });
  await settle();
  setId(2);
  flush();
  await settle();

  expect((errors[0] as Error).message).toBe("boom");
  expect(read()).toBe(1);
});

test("Loading shows its fallback until the first content and keeps the content while reloading", async () => {
  const [id, setId] = signal(1);
  const { requests, load } = requestsOf();
  async function User(props: { id: number }) {
    const name = await load(props.id);
    return <b>{name}</b>;
  }
  const { el } = mount(() => (
    <Loading fallback={<i>loading</i>}>
      <User id={id()} />
    </Loading>
  ));
  tick();
  expect(el.innerHTML).toBe("<i>loading</i>");

  requests[1].resolve("a");
  await settle();
  expect(el.innerHTML).toBe("<b>a</b>");

  setId(2);
  tick();
  expect(el.innerHTML).toBe("<b>a</b>");

  requests[2].resolve("b");
  await settle();
  expect(el.innerHTML).toBe("<b>b</b>");
});

test("Loading waits for every async component below it", async () => {
  const { requests, load } = requestsOf();
  async function User(props: { id: number }) {
    const name = await load(props.id);
    return <b>{name}</b>;
  }
  const { el } = mount(() => (
    <Loading fallback={<i>loading</i>}>
      <User id={1} />
      <User id={2} />
    </Loading>
  ));
  tick();
  requests[1].resolve("a");
  await settle();
  expect(el.innerHTML).toBe("<i>loading</i>");

  requests[2].resolve("b");
  await settle();
  expect(el.innerHTML).toBe("<b>a</b><b>b</b>");
});

test("Loading releases a component disposed before its first settle", async () => {
  const [on, setOn] = signal(true);
  const pending = new Map<string, PromiseWithResolvers<string>>();
  const load = (id: string): Promise<string> => {
    const request = Promise.withResolvers<string>();
    pending.set(id, request);
    return request.promise;
  };
  async function User(props: { id: string }) {
    const name = await load(props.id);
    return <b>{name}</b>;
  }
  const { el } = mount(() => (
    <Loading fallback={<i>loading</i>}>
      <div>
        <Show when={on()}>
          <User id="a" />
        </Show>
        <User id="b" />
      </div>
    </Loading>
  ));
  setOn(false);
  tick();
  pending.get("a")!.resolve("a");
  pending.get("b")!.resolve("b");
  await settle();
  expect(el.innerHTML).toBe("<div><b>b</b></div>");
});

test("Loading waits for a top-level Show", async () => {
  const { requests, load } = requestsOf();
  async function User(props: { id: number }) {
    const name = await load(props.id);
    return <b>{name}</b>;
  }
  const { el } = mount(() => (
    <Loading fallback={<i>loading</i>}>
      <Show when={true}>
        <User id={7} />
      </Show>
    </Loading>
  ));
  tick();
  expect(el.innerHTML).toBe("<i>loading</i>");
  requests[7].resolve("t");
  await settle();
  expect(el.innerHTML).toBe("<b>t</b>");
});

test("Loading never waits for its fallback", async () => {
  const pending = new Map<string, PromiseWithResolvers<string>>();
  const load = (id: string): Promise<string> => {
    const request = Promise.withResolvers<string>();
    pending.set(id, request);
    return request.promise;
  };
  async function User(props: { id: string }) {
    const name = await load(props.id);
    return <b>{name}</b>;
  }
  const { el } = mount(() => (
    <Loading fallback={<User id="spinner" />}>
      <User id="content" />
    </Loading>
  ));
  pending.get("content")!.resolve("content");
  await settle();
  expect(el.innerHTML).toBe("<b>content</b>");
});

test("Loading waits for an asyncComputed read by a sync component", async () => {
  const request = Promise.withResolvers<string>();
  const name = asyncComputed(() => request.promise);
  function Label() {
    return <b>{name.value()}</b>;
  }
  const { el } = mount(() => (
    <Loading fallback={<i>loading</i>}>
      <Label />
    </Loading>
  ));
  tick();
  expect(el.innerHTML).toBe("<i>loading</i>");
  request.resolve("x");
  await settle();
  expect(el.innerHTML).toBe("<b>x</b>");
});

test("a rejected load inside Loading reaches catchError and renders nothing", async () => {
  const request = Promise.withResolvers<string>();
  const errors: unknown[] = [];
  async function User() {
    const name = await request.promise;
    return <b>{name}</b>;
  }
  const { el } = mount(() =>
    catchError(
      () => (
        <Loading fallback={<i>loading</i>}>
          <User />
        </Loading>
      ),
      (error) => errors.push(error),
    ),
  );
  request.reject(new Error("nope"));
  await settle();
  expect((errors[0] as Error).message).toBe("nope");
  expect(el.innerHTML).toBe("");
});

test("Loading holds the old side of a Show until the new side loads", async () => {
  const [tab, setTab] = signal("a");
  const pending = new Map<string, PromiseWithResolvers<string>>();
  const load = (id: string): Promise<string> => {
    const request = Promise.withResolvers<string>();
    pending.set(id, request);
    return request.promise;
  };
  async function Tab(props: { id: string }) {
    const name = await load(props.id);
    return <b>{name}</b>;
  }
  const { el } = mount(() => (
    <Loading fallback={<i>loading</i>}>
      <div>
        <Show when={tab() === "a"} fallback={<Tab id="b" />}>
          <Tab id="a" />
        </Show>
      </div>
    </Loading>
  ));
  tick();
  expect(el.innerHTML).toBe("<i>loading</i>");
  pending.get("a")!.resolve("A");
  await settle();
  expect(el.innerHTML).toBe("<div><b>A</b></div>");

  setTab("b");
  tick();
  expect(el.innerHTML).toBe("<div><b>A</b></div>");

  pending.get("b")!.resolve("B");
  await settle();
  expect(el.innerHTML).toBe("<div><b>B</b></div>");
});

test("Loading keeps the old side when switching back before the new side loads", async () => {
  const [tab, setTab] = signal("a");
  const pending = new Map<string, PromiseWithResolvers<string>>();
  const load = (id: string): Promise<string> => {
    const request = Promise.withResolvers<string>();
    pending.set(id, request);
    return request.promise;
  };
  async function Tab(props: { id: string }) {
    const name = await load(props.id);
    return <b>{name}</b>;
  }
  const { el } = mount(() => (
    <Loading fallback={<i>loading</i>}>
      <div>
        <Show when={tab() === "a"} fallback={<Tab id="b" />}>
          <Tab id="a" />
        </Show>
      </div>
    </Loading>
  ));
  pending.get("a")!.resolve("A");
  await settle();
  expect(el.innerHTML).toBe("<div><b>A</b></div>");

  setTab("b");
  tick();
  expect(el.innerHTML).toBe("<div><b>A</b></div>");

  setTab("a");
  tick();
  expect(el.innerHTML).toBe("<div><b>A</b></div>");

  pending.get("b")!.resolve("B");
  await settle();
  expect(el.innerHTML).toBe("<div><b>A</b></div>");
});

test("an async component assimilates an inline thenable", async () => {
  async function Card() {
    const name = await { then: (resolve: (value: string) => void) => resolve("ann") };
    return <p>{name}</p>;
  }
  const { el } = mount(() => <Card />);
  expect(el.innerHTML).toBe("");
  await settle();
  expect(el.innerHTML).toBe("<p>ann</p>");
});

test("a plain async event handler keeps native await semantics", async () => {
  function Button() {
    const [label, setLabel] = signal("wait");
    return <button onClick={async () => setLabel(await Promise.resolve("ready"))}>{label()}</button>;
  }
  const { el } = mount(() => <Button />);
  el.querySelector("button")!.click();
  await settle();
  tick();
  expect(el.innerHTML).toBe("<button>ready</button>");
});
