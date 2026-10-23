import { cleanup, mount, settle, tick } from "@rezejs/testing-library";
import { Errored, Loading } from "reze-js";
import { afterEach, expect, test } from "vite-plus/test";

afterEach(cleanup);

type Load = (id: number) => Promise<string>;

async function Profile(props: { id: number; load: Load }) {
  const id = props.id;
  const load = props.load;
  const name = await load(id);
  return <p>{name}</p>;
}

function requestsOf(): { requests: PromiseWithResolvers<string>[]; load: Load } {
  const requests: PromiseWithResolvers<string>[] = [];
  const load: Load = () => {
    const request = Promise.withResolvers<string>();
    requests.push(request);
    return request.promise;
  };
  return { requests, load };
}

test("a rejected load shows the errored fallback, and its reset runs the load again", async () => {
  const { requests, load } = requestsOf();
  const { el } = mount(() => (
    <Errored fallback={(error, reset) => <button onClick={reset}>{String(error)}</button>}>
      <Loading fallback={<i>loading</i>}>
        <Profile id={1} load={load} />
      </Loading>
    </Errored>
  ));
  tick();
  expect(el.innerHTML).toBe("<i>loading</i>");

  requests[0].reject(new Error("offline"));
  await settle();
  expect(el.innerHTML).toBe("<button>Error: offline</button>");

  el.querySelector("button")!.click();
  tick();
  requests[1].resolve("ada");
  await settle();
  expect(el.innerHTML).toBe("<p>ada</p>");
});

test("loading reveals every async child below it at once", async () => {
  const { requests, load } = requestsOf();
  const { el } = mount(() => (
    <Loading fallback={<i>loading</i>}>
      <Profile id={1} load={load} />
      <Profile id={2} load={load} />
    </Loading>
  ));
  tick();

  requests[0].resolve("ada");
  await settle();
  expect(el.innerHTML).toBe("<i>loading</i>");

  requests[1].resolve("grace");
  await settle();
  expect(el.innerHTML).toBe("<p>ada</p><p>grace</p>");
});

test("an errored fallback element ignores the error", async () => {
  const { requests, load } = requestsOf();
  const { el } = mount(() => (
    <Errored fallback={<b>failed</b>}>
      <Profile id={1} load={load} />
    </Errored>
  ));
  tick();

  requests[0].reject(new Error("offline"));
  await settle();
  expect(el.innerHTML).toBe("<b>failed</b>");
});
