import { computed, signal } from "@rezejs/signals";
import { cleanup, mount, settle, tick } from "@rezejs/testing-library";
import { catchError, onCleanup } from "reze-js";
import { afterEach, expect, test } from "vite-plus/test";

afterEach(cleanup);

const message = (error: unknown): string => (error as Error).message;

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
