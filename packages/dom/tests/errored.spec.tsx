import { computed, signal } from "@rezejs/signals";
import { cleanup, mount, settle, tick } from "@rezejs/testing-library";
import { catchError, onCleanup } from "reze-js";
import { afterEach, expect, test } from "vitest";

afterEach(cleanup);

const message = (error: unknown): string => (error as Error).message;

test("a failure view shows the error when a child throws while building, and disposes what was built", async () => {
  const cleaned: string[] = [];
  function Half() {
    onCleanup(() => cleaned.push("half"));
    return (
      <b>
        {(() => {
          throw new Error("boom");
        })()}
      </b>
    );
  }
  async function Page() {
    await Promise.resolve();
    return <Half />;
  }
  Page.failure = (error: unknown) => <i>{message(error)}</i>;
  const { el } = mount(() => <Page />);
  await settle();
  expect(el.innerHTML).toBe("<i>boom</i>");
  expect(cleaned).toEqual(["half"]);
});

test("a failure view replaces the view when something built inside fails later, and ignores further failures", async () => {
  const cleaned: string[] = [];
  const [broken, setBroken] = signal(0);
  const label = computed(() => {
    if (broken() > 0) {
      throw new Error(`broken ${broken()}`);
    }
    return "ok";
  });
  function Child() {
    onCleanup(() => cleaned.push("child"));
    return (
      <>
        <b>{label()}</b>
        <u>{label()}</u>
      </>
    );
  }
  async function Page() {
    await Promise.resolve();
    return <Child />;
  }
  Page.failure = (error: unknown) => <i>{message(error)}</i>;
  const { el } = mount(() => <Page />);
  await settle();
  expect(el.innerHTML).toBe("<b>ok</b><u>ok</u>");
  setBroken(1);
  tick();
  expect(el.innerHTML).toBe("<i>broken 1</i>");
  expect(cleaned).toEqual(["child"]);
  setBroken(2);
  tick();
  expect(el.innerHTML).toBe("<i>broken 1</i>");
});

test("retry runs the component again, and a repeated failure shows the failure view again", async () => {
  let builds = 0;
  const [broken, setBroken] = signal(true);
  function Child() {
    builds++;
    if (broken()) {
      throw new Error("still broken");
    }
    return <b>ok</b>;
  }
  async function Page() {
    await Promise.resolve();
    return <Child />;
  }
  Page.failure = (error: unknown, retry: () => void) => <button onClick={retry}>{message(error)}</button>;
  const { el } = mount(() => <Page />);
  await settle();
  expect(el.innerHTML).toBe("<button>still broken</button>");
  expect(builds).toBe(1);
  el.querySelector("button")!.click();
  await settle();
  expect(el.innerHTML).toBe("<button>still broken</button>");
  expect(builds).toBe(2);
  setBroken(false);
  el.querySelector("button")!.click();
  await settle();
  expect(el.innerHTML).toBe("<b>ok</b>");
  expect(builds).toBe(3);
});

test("a rejected await in a component without pending shows its failure view", async () => {
  const request = Promise.withResolvers<string>();
  async function User() {
    const name = await request.promise;
    return <b>{name}</b>;
  }
  User.failure = (error: unknown) => <p>{message(error)}</p>;
  const { el } = mount(() => <User />);
  request.reject(new Error("nope"));
  await settle();
  expect(el.innerHTML).toBe("<p>nope</p>");
});

test("a failure view that returns nothing renders nothing", async () => {
  function Bomb(): never {
    throw new Error("boom");
  }
  async function Page() {
    await Promise.resolve();
    return <Bomb />;
  }
  Page.failure = () => undefined;
  const { el } = mount(() => <Page />);
  await settle();
  expect(el.innerHTML).toBe("");
});

test("an async component without a failure view passes its rejected await to the surrounding handler", async () => {
  const errors: unknown[] = [];
  const request = Promise.withResolvers<string>();
  async function User() {
    const name = await request.promise;
    return <b>{name}</b>;
  }
  mount(() =>
    catchError(
      () => <User />,
      (error) => errors.push(error),
    ),
  );
  request.reject(new Error("nope"));
  await settle();
  expect(errors.map(message)).toEqual(["nope"]);
});

test("an error thrown by the failure view goes to the surrounding handler", async () => {
  const errors: unknown[] = [];
  function Bomb(): never {
    throw new Error("first");
  }
  async function Page() {
    await Promise.resolve();
    return <Bomb />;
  }
  Page.failure = (error: unknown) => {
    throw new Error(`${message(error)} then second`);
  };
  mount(() =>
    catchError(
      () => <Page />,
      (error) => errors.push(error),
    ),
  );
  await settle();
  expect(errors.map(message)).toEqual(["first then second"]);
});
