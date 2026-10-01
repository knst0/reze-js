import { Errored, lazy, Loading } from "reze-js";
import { afterEach, expect, test } from "vitest";

import { cleanup, fire, mount, tick } from "../../../testing/dom";

afterEach(cleanup);

function settle(): Promise<void> {
  const { promise, resolve } = Promise.withResolvers<void>();
  setTimeout(resolve, 0);
  return promise;
}

function Greeting(props: { name: string }) {
  return <b>hi {props.name}</b>;
}

function deferred<M>(): { load: () => Promise<M>; calls: () => number; resolve: (module: M) => void; reject: (error: Error) => void } {
  let current = Promise.withResolvers<M>();
  let calls = 0;
  return {
    load: () => {
      calls++;
      return current.promise;
    },
    calls: () => calls,
    resolve: (module) => current.resolve(module),
    reject: (error) => {
      current.reject(error);
      current = Promise.withResolvers<M>();
    },
  };
}

test("a lazy component shows the Loading fallback until its module arrives, then renders with the props it was given", async () => {
  const source = deferred<{ default: typeof Greeting }>();
  const Late = lazy(source.load);
  const { el } = mount(() => (
    <Loading fallback={<i>wait</i>}>
      <Late name="ann" />
    </Loading>
  ));
  expect(el.innerHTML).toBe("<i>wait</i>");

  source.resolve({ default: Greeting });
  await settle();
  expect(el.innerHTML).toBe("<b>hi ann</b>");
});

test("the module is requested once, and instances created after it arrived render without a fallback", async () => {
  const source = deferred<{ default: typeof Greeting }>();
  const Late = lazy(source.load);
  source.resolve({ default: Greeting });
  const first = mount(() => (
    <Loading fallback={<i>wait</i>}>
      <Late name="a" />
    </Loading>
  ));
  const second = mount(() => (
    <Loading fallback={<i>wait</i>}>
      <Late name="b" />
    </Loading>
  ));
  await settle();
  const third = mount(() => <Late name="c" />);
  expect(first.el.innerHTML).toBe("<b>hi a</b>");
  expect(second.el.innerHTML).toBe("<b>hi b</b>");
  expect(third.el.innerHTML).toBe("<b>hi c</b>");
  expect(source.calls()).toBe(1);
});

test("a failed load goes to Errored, and its reset loads the module again", async () => {
  const source = deferred<{ default: typeof Greeting }>();
  const Late = lazy(source.load);
  const { el } = mount(() => (
    <Errored fallback={(error, reset) => <button onClick={reset}>{(error as Error).message}</button>}>
      <Loading>
        <Late name="ann" />
      </Loading>
    </Errored>
  ));
  source.reject(new Error("offline"));
  await settle();
  expect(el.innerHTML).toBe("<button>offline</button>");

  fire(el.querySelector("button")!, "click");
  tick();
  source.resolve({ default: Greeting });
  await settle();
  expect(el.innerHTML).toBe("<b>hi ann</b>");
  expect(source.calls()).toBe(2);
});

test("a named export is the component, and preload loads before the first render", async () => {
  const source = deferred<{ Greeting: typeof Greeting }>();
  const Late = lazy(source.load, { export: "Greeting" });
  const preloaded = Late.preload();
  source.resolve({ Greeting });
  await preloaded;
  await settle();
  const { el } = mount(() => <Late name="ann" />);
  expect(el.innerHTML).toBe("<b>hi ann</b>");
  expect(source.calls()).toBe(1);
});

test("a module without the named export reports that to Errored", async () => {
  const source = deferred<{ Missing: typeof Greeting }>();
  const Late = lazy(source.load, { export: "Missing" });
  const { el } = mount(() => (
    <Errored fallback={(error) => <em>{(error as Error).message}</em>}>
      <Loading>
        <Late name="ann" />
      </Loading>
    </Errored>
  ));
  source.resolve({} as { Missing: typeof Greeting });
  await settle();
  expect(el.innerHTML).toBe("<em>[reze] lazy: the loaded module has no component export `Missing`</em>");
});
