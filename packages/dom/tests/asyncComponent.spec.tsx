import { signal } from "@rezejs/signals";
import { cleanup, mount, settle, tick } from "@rezejs/testing-library";
import { catchError, computed, effect, flush, onCleanup, root, Show } from "reze-js";
import { asyncComponent } from "reze-js/internal/runtime";
import { afterEach, expect, test } from "vite-plus/test";

afterEach(cleanup);

function requestsOf(): { requests: Record<number, PromiseWithResolvers<string>>; load: (n: number) => Promise<string> } {
  const requests: Record<number, PromiseWithResolvers<string>> = {};
  const load = (n: number): Promise<string> => {
    requests[n] = Promise.withResolvers<string>();
    return requests[n].promise;
  };
  return { requests, load };
}

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
