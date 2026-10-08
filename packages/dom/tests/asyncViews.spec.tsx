import { cleanup, mount, settle, tick } from "@rezejs/testing-library";
import { afterEach, expect, test } from "vitest";

afterEach(cleanup);

type Load = (id: number) => Promise<string>;

async function Profile(props: { id: number; load: Load }) {
  const id = props.id;
  const load = props.load;
  const name = await load(id);
  return <p>{name}</p>;
}
Profile.pending = <i>loading</i>;
Profile.failure = (error: unknown, retry: () => void) => <button onClick={retry}>{String(error)}</button>;

function requestsOf(): { requests: PromiseWithResolvers<string>[]; load: Load } {
  const requests: PromiseWithResolvers<string>[] = [];
  const load: Load = () => {
    const request = Promise.withResolvers<string>();
    requests.push(request);
    return request.promise;
  };
  return { requests, load };
}

test("an async component shows its pending view until its first load settles, then its body", async () => {
  const { requests, load } = requestsOf();
  const { el } = mount(() => <Profile id={1} load={load} />);
  tick();
  expect(el.innerHTML).toBe("<i>loading</i>");

  requests[0].resolve("ada");
  await settle();
  expect(el.innerHTML).toBe("<p>ada</p>");
});

test("a rejected load shows the failure view, and its retry runs the load again", async () => {
  const { requests, load } = requestsOf();
  const { el } = mount(() => <Profile id={1} load={load} />);
  tick();

  requests[0].reject(new Error("offline"));
  await settle();
  expect(el.innerHTML).toBe("<button>Error: offline</button>");

  el.querySelector("button")!.click();
  tick();
  requests[1].resolve("ada");
  await settle();
  expect(el.innerHTML).toBe("<p>ada</p>");
});
