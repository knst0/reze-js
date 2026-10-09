import { signal } from "@rezejs/signals";
import { cleanup, mount, settle, tick } from "@rezejs/testing-library";
import { abortSignal, isPending } from "reze-js";
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

test("isPending is false during the first load and true while a reload is pending", async () => {
  const [id, setId] = signal(1);
  const { requests, load } = requestsOf();
  async function Card(props: { id: number }) {
    const userId = props.id;
    const name = await load(userId);
    return <p>{isPending() ? "refreshing" : name}</p>;
  }
  const { el } = mount(() => <Card id={id()} />);
  tick();

  requests[1].resolve("a");
  await settle();
  expect(el.innerHTML).toBe("<p>a</p>");

  setId(2);
  tick();
  expect(el.innerHTML).toBe("<p>refreshing</p>");

  requests[2].resolve("b");
  await settle();
  expect(el.innerHTML).toBe("<p>b</p>");
});

test("abortSignal aborts the run a reload supersedes, and leaves the current run's signal live", async () => {
  const [id, setId] = signal(1);
  const { requests, load } = requestsOf();
  const signals: AbortSignal[] = [];
  async function Card(props: { id: number }) {
    const userId = props.id;
    const run = abortSignal();
    signals.push(run);
    const name = await load(userId);
    return <p>{name}</p>;
  }
  const { el } = mount(() => <Card id={id()} />);
  tick();

  setId(2);
  tick();
  expect(signals[0].aborted).toBe(true);
  expect(signals[1].aborted).toBe(false);

  requests[2].resolve("b");
  await settle();
  expect(el.innerHTML).toBe("<p>b</p>");
  expect(signals[1].aborted).toBe(false);
});
