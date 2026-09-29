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

test("an async component renders once its await settles, keeps its content while reloading, and holds state built after the await", async () => {
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

  setId(2);
  tick();
  expect(el.innerHTML).toBe("<button>a:1</button>");

  requests[2].resolve("b");
  await settle();
  expect(el.innerHTML).toBe("<button>b:0</button>");
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
      ([n]) => {
        bodies.push(`${n}${label()}`);
        return n;
      },
    );
    effect(() => {
      seen.push(child());
    });
  });
  await settle();
  setLabel("b");
  flush();
  setId(2);
  flush();
  await settle();

  expect(seen).toEqual([undefined, 10, 20]);
  expect(bodies).toEqual(["10a", "20b"]);
});

test("a new load disposes what the previous body created", async () => {
  const [id, setId] = signal(1);
  const cleaned: number[] = [];
  root(() => {
    const child = asyncComponent(
      async (c) => [c.get(id)],
      ([n]) => {
        onCleanup(() => cleaned.push(n!));
        return n;
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

  expect(cleaned).toEqual([1]);
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
          ([n]) => n,
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
