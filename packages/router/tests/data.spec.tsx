import { flush } from "@rezejs/signals";
import { cleanup, fire, mount } from "@rezejs/test-utils";
import { afterEach, expect, test } from "vitest";

import {
  action,
  createRouter,
  defineRoute,
  defineRoutes,
  memoryHistory,
  query,
  revalidate,
  useAction,
  useSubmissions,
  type RouteDefinition,
  type Submission,
} from "../src";

afterEach(() => {
  cleanup();
  query.clear();
});

async function settle(rounds = 3): Promise<void> {
  for (let round = 0; round < rounds; round++) {
    await new Promise((resolve) => setTimeout(resolve, 0));
    flush();
  }
}

function shell(route: RouteDefinition) {
  const history = memoryHistory("/");
  const App = createRouter({ history, routes: defineRoutes([route]) });
  return { history, App };
}

test("query caches by name and arguments", async () => {
  let calls = 0;
  const getUser = query(async (id: string) => {
    calls++;
    return `user-${id}`;
  }, "users");
  expect(await getUser("1")).toBe("user-1");
  expect(await getUser("1")).toBe("user-1");
  expect(calls).toBe(1);
  expect(await getUser("2")).toBe("user-2");
  expect(calls).toBe(2);
  expect(getUser.key).toBe("users");
  expect(getUser.keyFor("1")).toBe(getUser.keyFor("1"));
  expect(getUser.keyFor("1")).not.toBe(getUser.keyFor("2"));
});

test("query object keys serialize stably", () => {
  const search = query(async (params: { q: string; page: number }) => params.q, "search");
  expect(search.keyFor({ q: "x", page: 1 })).toBe(search.keyFor({ page: 1, q: "x" }));
});

test("query cache methods read and write settled values", async () => {
  const getValue = query(async () => "live", "values");
  expect(() => query.get("missing")).toThrow();
  await getValue();
  expect(query.get(getValue.keyFor())).toBe("live");
  query.set(getValue.keyFor(), "seeded");
  expect(query.get(getValue.keyFor())).toBe("seeded");
  expect(query.delete(getValue.keyFor())).toBe(true);
  expect(query.delete(getValue.keyFor())).toBe(false);
  await getValue();
  query.clear();
  expect(() => query.get(getValue.keyFor())).toThrow();
});

test("revalidate retriggers tracked readers", async () => {
  let calls = 0;
  const getCount = query(async () => ++calls, "count");
  async function Counter() {
    const value = await getCount();
    return <p>{value}</p>;
  }
  const { App } = shell(defineRoute({ path: "/", component: Counter }));
  const { el } = mount(() => <App />);
  await settle();
  expect(el.innerHTML).toBe("<p>1</p>");
  revalidate("count");
  await settle();
  expect(el.innerHTML).toBe("<p>2</p>");
});

test("a redirect response navigates inside a router", async () => {
  const gate = query(
    async () => new Response(null, { status: 302, headers: { Location: "/login" } }),
    "gate",
  );
  async function Gate() {
    await gate();
    return <p>gate</p>;
  }
  const history = memoryHistory("/gate");
  const App = createRouter({
    history,
    routes: defineRoutes([
      defineRoute({ path: "/gate", component: Gate }),
      defineRoute({ path: "/login", component: () => <p>login</p> }),
    ]),
  });
  const { el } = mount(() => <App />);
  await settle(5);
  expect(history.get()).toBe("/login");
  expect(el.innerHTML).toBe("<p>login</p>");
});

test("an X-Revalidate header invalidates matching keys", async () => {
  let calls = 0;
  const getData = query(async () => ++calls, "data");
  await getData();
  expect(calls).toBe(1);
  const invalidate = action(
    async () =>
      new Response(JSON.stringify({ ok: true }), {
        headers: { "content-type": "application/json", "X-Revalidate": "data" },
      }),
    "invalidate",
  );
  await invalidate();
  await getData();
  expect(calls).toBe(2);
});

test("actions run directly and report results", async () => {
  const double = action(async (value: number) => value * 2, "double");
  expect(await double(21)).toBe(42);
  expect(double.url).toBe("https://action/double");
  expect(String(double)).toBe("https://action/double");
});

test("with binds leading arguments into the url", async () => {
  const add = action(async (a: number, b: number) => a + b, "add");
  const addOne = add.with(1);
  expect(await addOne(2)).toBe(3);
  expect(addOne.url).toContain("args");
  expect(addOne.url).not.toBe(add.url);
});

test("submit and settled hooks observe invocations", async () => {
  const seen: string[] = [];
  const save = action(async (value: string) => `saved-${value}`, "save");
  save.onSubmit((value) => void seen.push(`submit:${value}`));
  save.onSettled((submission) => void seen.push(`settled:${submission.result as string}`));
  expect(await save("a")).toBe("saved-a");
  expect(seen).toEqual(["submit:a", "settled:saved-a"]);
});

test("useAction and useSubmissions track settled records in a router", async () => {
  const { App } = shell(defineRoute({ path: "/", component: () => <p>home</p> }));
  const save = action(async (value: string) => `saved-${value}`, "tracked-save");
  let run!: (value: string) => Promise<string>;
  let records!: () => number;
  let first!: () => unknown;
  mount(() => (
    <App>
      {() => {
        const bound = useAction(save);
        const submissions = useSubmissions(save);
        run = bound;
        records = () => submissions.length;
        first = () => submissions[0]?.result;
        return <p>home</p>;
      }}
    </App>
  ));
  flush();
  expect(records!()).toBe(0);
  await run!("a");
  await settle();
  expect(records!()).toBe(1);
  expect(first!()).toBe("saved-a");
});

test("submission retry reruns with the same input and clear empties", async () => {
  const { App } = shell(defineRoute({ path: "/", component: () => <p>home</p> }));
  let calls = 0;
  const bump = action(async () => ++calls, "bump");
  let submissions!: Submission<[], number>[];
  let run!: () => Promise<number>;
  mount(() => (
    <App>
      {() => {
        run = useAction(bump);
        submissions = useSubmissions(bump);
        return <p>home</p>;
      }}
    </App>
  ));
  flush();
  await run!();
  await settle();
  expect(submissions!.length).toBe(1);
  await submissions![0]!.retry();
  await settle();
  expect(calls).toBe(2);
  expect(submissions!.length).toBe(1);
  submissions![0]!.clear();
  flush();
  expect(submissions!.length).toBe(0);
});

test("action errors reject bound calls and record submissions", async () => {
  const { App } = shell(defineRoute({ path: "/", component: () => <p>home</p> }));
  const fail = action(async (): Promise<string> => {
    throw new Error("nope");
  }, "fail");
  await expect(fail()).rejects.toThrow("nope");
  let submissions!: Submission<[], string>[];
  let run!: () => Promise<string>;
  mount(() => (
    <App>
      {() => {
        run = useAction(fail);
        submissions = useSubmissions(fail);
        return <p>home</p>;
      }}
    </App>
  ));
  flush();
  await expect(run!()).rejects.toThrow("nope");
  await settle();
  expect(submissions!.length).toBe(1);
  expect((submissions![0]!.error as Error).message).toBe("nope");
});

test("delegated form submits run the action with busy state", async () => {
  const { App } = shell(defineRoute({ path: "/", component: () => <p>home</p> }));
  const gate = Promise.withResolvers<string>();
  const save = action(async (data: FormData | URLSearchParams) => {
    const value = await gate.promise;
    const name = data.get("name");
    return `${typeof name === "string" ? name : ""}:${value}`;
  }, "form-save");
  let submissions!: Submission<[FormData | URLSearchParams], string>[];
  const { el } = mount(() => (
    <App>
      {() => {
        submissions = useSubmissions(save);
        return (
          <form action={save} method="post">
            <input name="name" value="ada" />
            <button>save</button>
          </form>
        );
      }}
    </App>
  ));
  const form = el.querySelector("form")!;
  expect(form.getAttribute("action")).toBe("https://action/form-save");
  fire(form, "submit");
  await settle(1);
  expect(form.getAttribute("aria-busy")).toBe("true");
  gate.resolve("done");
  await settle();
  expect(form.hasAttribute("aria-busy")).toBe(false);
  expect(submissions!.length).toBe(1);
  expect(submissions![0]!.result).toBe("ada:done");
});

test("void actions leave no submission behind", async () => {
  const { App } = shell(defineRoute({ path: "/", component: () => <p>home</p> }));
  const ping = action(async () => {}, "ping");
  let submissions!: Submission<[], void>[];
  let run!: () => Promise<void>;
  mount(() => (
    <App>
      {() => {
        run = useAction(ping);
        submissions = useSubmissions(ping);
        return <p>home</p>;
      }}
    </App>
  ));
  flush();
  await run!();
  await settle();
  expect(submissions!.length).toBe(0);
});

test("useSubmissions filters by input", async () => {
  const { App } = shell(defineRoute({ path: "/", component: () => <p>home</p> }));
  const save = action(async (value: string) => value, "filtered-save");
  let count!: () => number;
  mount(() => (
    <App>
      {() => {
        const submissions = useSubmissions(save, (input) => input[0] === "keep");
        count = () => submissions.length;
        void useAction(save)("drop");
        return <p>home</p>;
      }}
    </App>
  ));
  await settle(5);
  expect(count!()).toBe(0);
});
