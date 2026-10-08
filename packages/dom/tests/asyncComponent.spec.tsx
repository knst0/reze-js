import { signal } from "@rezejs/signals";
import { cleanup, mount, settle, tick } from "@rezejs/testing-library";
import { catchError, computed, effect, flush, onCleanup, root, Show } from "reze-js";
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
    const userId = props.id;
    const name = await load(userId);
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
    const prefix = props.prefix;
    const base = await echo(prefix);
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

type Load = (id: number) => Promise<string>;

async function Name(props: { id: number; load: Load }) {
  const id = props.id;
  const load = props.load;
  const name = await load(id);
  return <b>{name}</b>;
}
Name.pending = <i>loading</i>;

async function Item(props: { id: number; load: Load }) {
  const id = props.id;
  const load = props.load;
  const name = await load(id);
  return <b>{name}</b>;
}

async function Stuck() {
  await new Promise<never>(() => {});
  return <i>spin</i>;
}

async function Patient(props: { load: Load }) {
  const load = props.load;
  const name = await load(2);
  return <b>{name}</b>;
}
Patient.pending = <Stuck />;

async function Page(props: { load: Load }) {
  const load = props.load;
  await Promise.resolve();
  return (
    <div>
      <Item id={1} load={load} />
      <Item id={2} load={load} />
    </div>
  );
}
Page.pending = <i>loading</i>;

async function Gate(props: { on: () => boolean; load: Load }) {
  const on = props.on;
  const load = props.load;
  await Promise.resolve();
  return (
    <div>
      <Show when={on()}>
        <Item id={1} load={load} />
      </Show>
      <Item id={2} load={load} />
    </div>
  );
}
Gate.pending = <i>loading</i>;

async function Wrapped(props: { load: Load }) {
  const load = props.load;
  await Promise.resolve();
  return (
    <Show when={true}>
      <Item id={7} load={load} />
    </Show>
  );
}
Wrapped.pending = <i>loading</i>;

async function Badge(props: { load: () => Promise<string> }) {
  const load = props.load;
  const name = computed(await load());
  return <b>{name}</b>;
}
Badge.pending = <i>loading</i>;

async function Tabs(props: { tab: () => string; load: Load }) {
  const tab = props.tab;
  const load = props.load;
  await Promise.resolve();
  return (
    <div>
      <Show when={tab() === "a"} fallback={<Item id={2} load={load} />}>
        <Item id={1} load={load} />
      </Show>
    </div>
  );
}
Tabs.pending = <i>loading</i>;

test("a pending view shows until the first content settles and keeps the content while a reload is pending", async () => {
  const [id, setId] = signal(1);
  const { requests, load } = requestsOf();
  const { el } = mount(() => <Name id={id()} load={load} />);
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

test("a component without pending holds the pending view of its ancestor until its own load settles", async () => {
  const { requests, load } = requestsOf();
  const { el } = mount(() => <Page load={load} />);
  await settle();
  expect(el.innerHTML).toBe("<i>loading</i>");

  requests[1].resolve("a");
  await settle();
  expect(el.innerHTML).toBe("<i>loading</i>");

  requests[2].resolve("b");
  await settle();
  expect(el.innerHTML).toBe("<div><b>a</b><b>b</b></div>");
});

test("a component disposed before its first settle releases the pending view that was waiting on it", async () => {
  const [on, setOn] = signal(true);
  const { requests, load } = requestsOf();
  const { el } = mount(() => <Gate on={on} load={load} />);
  await settle();
  expect(el.innerHTML).toBe("<i>loading</i>");

  setOn(false);
  tick();
  requests[2].resolve("b");
  await settle();
  expect(el.innerHTML).toBe("<div><b>b</b></div>");
});

test("a pending view waits for a top-level Show's content", async () => {
  const { requests, load } = requestsOf();
  const { el } = mount(() => <Wrapped load={load} />);
  await settle();
  expect(el.innerHTML).toBe("<i>loading</i>");

  requests[7].resolve("t");
  await settle();
  expect(el.innerHTML).toBe("<b>t</b>");
});

test("a pending view never holds the component it stands in for", async () => {
  const { requests, load } = requestsOf();
  const { el } = mount(() => <Patient load={load} />);
  await settle();

  requests[2].resolve("content");
  await settle();
  expect(el.innerHTML).toBe("<b>content</b>");
});

test("a read of an async computed holds the pending view until it settles", async () => {
  const request = Promise.withResolvers<string>();
  const { el } = mount(() => <Badge load={() => request.promise} />);
  await settle();
  expect(el.innerHTML).toBe("<i>loading</i>");

  request.resolve("x");
  await settle();
  expect(el.innerHTML).toBe("<b>x</b>");
});

test("a rejected load with no failure view reaches catchError and renders nothing", async () => {
  const { requests, load } = requestsOf();
  const errors: unknown[] = [];
  const { el } = mount(() =>
    catchError(
      () => <Name id={1} load={load} />,
      (error) => errors.push(error),
    ),
  );
  await settle();

  requests[1].reject(new Error("nope"));
  await settle();
  expect((errors[0] as Error).message).toBe("nope");
  expect(el.innerHTML).toBe("");
});

test("a pending component holds the shown side of a Show until the new side loads", async () => {
  const [tab, setTab] = signal("a");
  const { requests, load } = requestsOf();
  const { el } = mount(() => <Tabs tab={tab} load={load} />);
  await settle();
  expect(el.innerHTML).toBe("<i>loading</i>");

  requests[1].resolve("A");
  await settle();
  expect(el.innerHTML).toBe("<div><b>A</b></div>");

  setTab("b");
  tick();
  expect(el.innerHTML).toBe("<div><b>A</b></div>");

  requests[2].resolve("B");
  await settle();
  expect(el.innerHTML).toBe("<div><b>B</b></div>");
});

test("a pending component keeps the shown side when a Show switches back before the new side loads", async () => {
  const [tab, setTab] = signal("a");
  const { requests, load } = requestsOf();
  const { el } = mount(() => <Tabs tab={tab} load={load} />);
  await settle();

  requests[1].resolve("A");
  await settle();
  expect(el.innerHTML).toBe("<div><b>A</b></div>");

  setTab("b");
  tick();
  setTab("a");
  tick();
  expect(el.innerHTML).toBe("<div><b>A</b></div>");

  requests[2].resolve("B");
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
