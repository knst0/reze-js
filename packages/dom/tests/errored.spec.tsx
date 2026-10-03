import { cleanup, mount, settle, tick } from "@rezejs/testing-library";
import { catchError, computed, Errored, Loading, onCleanup, signal } from "reze-js";
import { afterEach, expect, test } from "vitest";

afterEach(cleanup);

const message = (error: unknown): string => (error as Error).message;

test("Errored shows the fallback with the error when a child throws while building, and disposes what was built", () => {
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
  const { el } = mount(() => (
    <Errored fallback={(error) => <i>{message(error)}</i>}>
      <Half />
    </Errored>
  ));
  expect(el.innerHTML).toBe("<i>boom</i>");
  expect(cleaned).toEqual(["half"]);
});

test("Errored swaps to the fallback when something built inside fails later, and ignores further failures", () => {
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
  const { el } = mount(() => (
    <Errored fallback={(error) => <i>{message(error)}</i>}>
      <Child />
    </Errored>
  ));
  expect(el.innerHTML).toBe("<b>ok</b><u>ok</u>");
  setBroken(1);
  tick();
  expect(el.innerHTML).toBe("<i>broken 1</i>");
  expect(cleaned).toEqual(["child"]);
  setBroken(2);
  tick();
  expect(el.innerHTML).toBe("<i>broken 1</i>");
});

test("reset builds the children again, and a repeated failure shows the fallback again", () => {
  let builds = 0;
  const [broken, setBroken] = signal(true);
  function Child() {
    builds++;
    if (broken()) {
      throw new Error("still broken");
    }
    return <b>ok</b>;
  }
  const { el } = mount(() => (
    <Errored fallback={(error, reset) => <button onClick={reset}>{message(error)}</button>}>
      <Child />
    </Errored>
  ));
  expect(el.innerHTML).toBe("<button>still broken</button>");
  expect(builds).toBe(1);
  el.querySelector("button")!.click();
  tick();
  expect(el.innerHTML).toBe("<button>still broken</button>");
  expect(builds).toBe(2);
  setBroken(false);
  el.querySelector("button")!.click();
  tick();
  expect(el.innerHTML).toBe("<b>ok</b>");
  expect(builds).toBe(3);
});

test("Errored renders nothing without a fallback, and a static fallback element ignores the error", () => {
  function Bomb(): never {
    throw new Error("boom");
  }
  const bare = mount(() => (
    <Errored>
      <Bomb />
    </Errored>
  ));
  expect(bare.el.innerHTML).toBe("");
  const fixed = mount(() => (
    <Errored fallback={<i>failed</i>}>
      <Bomb />
    </Errored>
  ));
  expect(fixed.el.innerHTML).toBe("<i>failed</i>");
});

test("Errored catches the rejection of an async component inside Loading", async () => {
  const request = Promise.withResolvers<string>();
  async function User() {
    const name = await request.promise;
    return <b>{name}</b>;
  }
  const { el } = mount(() => (
    <Loading fallback={<i>loading</i>}>
      <Errored fallback={(error) => <p>{message(error)}</p>}>
        <User />
      </Errored>
    </Loading>
  ));
  expect(el.innerHTML).toBe("<i>loading</i>");
  request.reject(new Error("nope"));
  await settle();
  expect(el.innerHTML).toBe("<p>nope</p>");
});

test("an error thrown by the fallback goes to the surrounding handler", () => {
  const errors: unknown[] = [];
  function Bomb(): never {
    throw new Error("first");
  }
  mount(() =>
    catchError(
      () => (
        <Errored
          fallback={(error) => {
            throw new Error(`${message(error)} then second`);
          }}
        >
          <Bomb />
        </Errored>
      ),
      (error) => errors.push(error),
    ),
  );
  expect(errors.map(message)).toEqual(["first then second"]);
});
