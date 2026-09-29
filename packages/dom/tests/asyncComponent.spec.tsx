import { asyncComponent, catchError, effect, flush, Loading, onCleanup, root, signal } from "reze-js";
import { afterEach, expect, test } from "vitest";

import { cleanup, mount, tick } from "../../../testing/dom";

afterEach(cleanup);

function settle(): Promise<void> {
  const { promise, resolve } = Promise.withResolvers<void>();
  setTimeout(resolve, 0);
  return promise;
}

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
