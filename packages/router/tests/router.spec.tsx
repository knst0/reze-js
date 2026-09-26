import { lazy, renderToString } from "@rezejs/dom";
import type { JSX } from "@rezejs/dom/jsx-runtime";
import { flush } from "@rezejs/signals";
import { cleanup, fire, mount } from "@rezejs/test-utils";
import { afterEach, beforeEach, expect, test } from "vitest";

import {
  createRouter,
  defineRoute,
  defineRoutes,
  int,
  memoryHistory,
  useBeforeLeave,
  useHref,
  useIsRouting,
  useLinkState,
  useLocation,
  useMatch,
  useNavigate,
  useParams,
  usePreloadRoute,
  useResolvedPath,
  useRouteMatches,
  useSearchParams,
  type RouteSectionProps,
} from "../src";

afterEach(cleanup);
beforeEach(() => window.history.replaceState(null, "", "/"));

async function settle(rounds = 3): Promise<void> {
  for (let round = 0; round < rounds; round++) {
    await new Promise((resolve) => setTimeout(resolve, 0));
    flush();
  }
}

const Home = () => <h1>home</h1>;
const Users = (props: { children?: JSX.Element }) => (
  <section>
    <h1>users</h1>
    {props.children}
  </section>
);
const User = () => {
  const params = useParams();
  return <p>user {params.id}</p>;
};
const NotFound = () => <p>missing {useLocation().pathname}</p>;

const routes = defineRoutes([
  defineRoute({ path: "/", component: Home }),
  defineRoute({
    path: "/users",
    component: Users,
    children: [
      defineRoute({ path: ":id", component: User }),
      defineRoute({ path: "new", component: () => <p>new user</p> }),
    ],
  }),
  defineRoute({ path: "/*rest", component: NotFound }),
]);

function app(initial = "/") {
  const history = memoryHistory(initial);
  return { history, App: createRouter({ routes, history }) };
}

test("renders the location from history with nested children", () => {
  const { App } = app("/users/7");
  expect(mount(() => <App />).el.innerHTML).toBe("<section><h1>users</h1><p>user 7</p></section>");
});

test("static beats param beats wildcard", () => {
  const { App } = app("/users/new");
  const { el } = mount(() => <App />);
  expect(el.innerHTML).toBe("<section><h1>users</h1><p>new user</p></section>");
  cleanup();
  const fallback = app("/nope/deep");
  expect(mount(() => <fallback.App />).el.innerHTML).toBe("<p>missing /nope/deep</p>");
});

test("an index child renders at the exact parent path", () => {
  const history = memoryHistory("/users");
  const App = createRouter({
    history,
    routes: defineRoutes([
      defineRoute({
        path: "/users",
        component: (props: RouteSectionProps) => <section>{props.children}</section>,
        children: [
          defineRoute({ path: "/", component: () => <p>pick one</p> }),
          defineRoute({ path: ":id", component: User }),
        ],
      }),
    ]),
  });
  expect(mount(() => <App />).el.innerHTML).toBe("<section><p>pick one</p></section>");
  cleanup();
  const Detail = createRouter({
    history: memoryHistory("/users/7"),
    routes: defineRoutes([
      defineRoute({
        path: "/users",
        component: (props: RouteSectionProps) => <section>{props.children}</section>,
        children: [
          defineRoute({ path: "/", component: () => <p>pick one</p> }),
          defineRoute({ path: ":id", component: User }),
        ],
      }),
    ]),
  });
  expect(mount(() => <Detail />).el.innerHTML).toBe("<section><p>user 7</p></section>");
});

test("match returns root-to-leaf output matches", () => {
  const { App } = app();
  expect(App.match("/users/7")).toEqual([
    {
      path: "/users",
      pattern: "/users",
      match: "/users",
      params: {},
      info: undefined,
    },
    {
      path: ":id",
      pattern: "/users/:id",
      match: "/users/7",
      params: { id: "7" },
      info: undefined,
    },
  ]);
  expect(App.match("/nothing")).toHaveLength(1);
  expect(App.match("/users/7").length).toBe(2);
});

test("optional params, path arrays, pathless layouts and match filters", () => {
  const history = memoryHistory("/");
  const Filtered = createRouter({
    history,
    routes: defineRoutes([
      defineRoute({
        path: "/posts/:page?",
        component: () => <p>page {useParams().page ?? "1"}</p>,
      }),
      defineRoute({ path: ["/a", "/b"], component: () => <p>ab</p> }),
      defineRoute({
        component: (props: { children?: JSX.Element }) => <main>{props.children}</main>,
        children: [defineRoute({ path: "/inner", component: () => <p>inner</p> })],
      }),
      defineRoute({
        path: "/n/:id",
        matchFilters: { id: int },
        component: () => <p>num {useParams().id}</p>,
      }),
      defineRoute({ path: "/*rest", component: () => <p>lost</p> }),
    ]),
  });
  const { el } = mount(() => <Filtered />);
  const go = (to: string) => {
    history.set({ value: to });
    flush();
  };
  expect(el.innerHTML).toBe("<p>lost</p>");
  go("/posts");
  expect(el.innerHTML).toBe("<p>page 1</p>");
  go("/posts/3");
  expect(el.innerHTML).toBe("<p>page 3</p>");
  go("/b");
  expect(el.innerHTML).toBe("<p>ab</p>");
  go("/inner");
  expect(el.innerHTML).toBe("<main><p>inner</p></main>");
  go("/n/42");
  expect(el.innerHTML).toBe("<p>num 42</p>");
  go("/n/abc");
  expect(el.innerHTML).toBe("<p>lost</p>");
});

test("navigate resolves against the route and writes history", async () => {
  const { App, history } = app("/");
  let navigate!: ReturnType<typeof useNavigate>;
  const { el } = mount(() => (
    <App>
      {(props) => {
        navigate = useNavigate();
        return props.children;
      }}
    </App>
  ));
  expect(el.innerHTML).toBe("<h1>home</h1>");
  navigate("/users/7");
  await settle();
  expect(el.innerHTML).toBe("<section><h1>users</h1><p>user 7</p></section>");
  expect(history.get()).toBe("/users/7");
  navigate("8");
  await settle();
  expect(el.querySelector("p")!.textContent).toBe("user 8");
  navigate(-2);
  await settle();
  expect(el.innerHTML).toBe("<h1>home</h1>");
});

test("plain anchor clicks navigate, other clicks stay native", async () => {
  const { App, history } = app("/");
  const { el } = mount(() => (
    <App>
      {(props) => (
        <nav>
          <a href="/users/1">one</a>
          <a href="https://example.com/">external</a>
          <a href="/users/2" target="_blank">
            blank
          </a>
          {props.children}
        </nav>
      )}
    </App>
  ));
  const [one, external, blank] = [...el.querySelectorAll("a")];
  expect(fire(one!, "click")).toBe(false);
  await settle();
  expect(history.get()).toBe("/users/1");
  expect(el.querySelector("p")!.textContent).toBe("user 1");
  const outbound = new MouseEvent("click", { bubbles: true, cancelable: true });
  external!.dispatchEvent(outbound);
  expect(outbound.defaultPrevented).toBe(false);
  const modified = new MouseEvent("click", { bubbles: true, cancelable: true, ctrlKey: true });
  blank!.dispatchEvent(modified);
  expect(modified.defaultPrevented).toBe(false);
});

test("links carry active state and pending state while routing", async () => {
  const history = memoryHistory("/");
  let gate!: PromiseWithResolvers<{ default: () => JSX.Element }>;
  gate = Promise.withResolvers();
  const Slow = lazy(() => gate.promise);
  const App = createRouter({
    history,
    routes: defineRoutes([
      defineRoute({ path: "/", component: Home }),
      defineRoute({ path: "/slow", component: Slow }),
    ]),
  });
  const { el } = mount(() => (
    <App>
      {(props) => (
        <>
          <nav>
            <a href="/slow">slow</a>
            <a href="/">home</a>
          </nav>
          {props.children}
        </>
      )}
    </App>
  ));
  const [slow, home] = [...el.querySelectorAll("a")];
  await settle();
  expect(home!.getAttribute("aria-current")).toBe("page");
  expect(home!.hasAttribute("data-active")).toBe(true);
  fire(slow!, "click");
  await settle(1);
  expect(slow!.hasAttribute("data-pending")).toBe(true);
  gate.resolve({ default: () => <p>slow page</p> });
  await settle();
  expect(el.querySelector("p")!.textContent).toBe("slow page");
  expect(slow!.hasAttribute("data-pending")).toBe(false);
  expect(slow!.getAttribute("aria-current")).toBe("page");
});

test("useMatch tests a pattern without consulting the tree", () => {
  const { App } = app("/docs/a/b");
  let seen!: { rest?: string };
  mount(() => (
    <App>
      {() => {
        const match = useMatch(() => "/docs/*rest");
        seen = { rest: match()?.params.rest };
        return <p>docs</p>;
      }}
    </App>
  ));
  flush();
  expect(seen.rest).toBe("a/b");
});

test("search params read raw and write merged", async () => {
  const history = memoryHistory("/search?q=a");
  const App = createRouter({
    history,
    routes: defineRoutes([defineRoute({ path: "/search", component: () => <p>hit</p> })]),
  });
  let read!: () => string | undefined;
  let write!: (params: Record<string, string>) => void;
  mount(() => (
    <App>
      {() => {
        const [params, setParams] = useSearchParams();
        read = () => params.q as string | undefined;
        write = (next) => setParams(next, {});
        return <p>hit</p>;
      }}
    </App>
  ));
  flush();
  expect(read!()).toBe("a");
  write!({ q: "b", page: "2" });
  await settle();
  expect(history.get()).toBe("/search?q=b&page=2");
  flush();
  expect(read!()).toBe("b");
});

test("route matches, resolved paths, hrefs and link state read the chain", async () => {
  const { App } = app("/users/7?tab=info");
  let snapshot!: {
    patterns: string[];
    resolved: string | undefined;
    link: { active: boolean; current: boolean; pending: boolean };
  };
  const Probe = () => {
    const matches = useRouteMatches();
    const resolved = useResolvedPath(() => "settings");
    const href = useHref(() => "/users/7");
    const link = useLinkState(() => "/users/7");
    const location = useLocation();
    snapshot = {
      patterns: matches().map((match) => match.route.pattern),
      resolved: resolved(),
      link: { active: link.active(), current: link.current(), pending: link.pending() },
    };
    void href;
    void location;
    return <p>probe</p>;
  };
  const Probed = createRouter({
    history: memoryHistory("/users/7?tab=info"),
    routes: defineRoutes([
      defineRoute({
        path: "/users",
        component: (props: RouteSectionProps) => <>{props.children}</>,
        children: [defineRoute({ path: ":id", component: Probe })],
      }),
    ]),
  });
  mount(() => <Probed />);
  flush();
  expect(snapshot.patterns).toEqual(["/users", "/users/:id"]);
  expect(snapshot.resolved).toBe("/users/7/settings");
  expect(snapshot.link).toEqual({ active: true, current: true, pending: false });
  void App;
});

test("preload data reaches the component and the root render prop", async () => {
  const seen: string[] = [];
  const App = createRouter({
    history: memoryHistory("/users/7"),
    preload: () => "root-data",
    routes: defineRoutes([
      defineRoute({
        path: "/users/:id",
        preload: ({ params }) => {
          seen.push(params.id!);
          return `user-${params.id}`;
        },
        component: (props: RouteSectionProps<string>) => <p>{props.data}</p>,
      }),
    ]),
  });
  let rootData!: unknown;
  const { el } = mount(() => (
    <App>
      {(props) => {
        rootData = props.data;
        return props.children;
      }}
    </App>
  ));
  await settle();
  expect(el.innerHTML).toBe("<p>user-7</p>");
  expect(rootData).toBe("root-data");
  expect(seen).toEqual(["7"]);
});

test("isRouting reports pending navigation and lazy resolution", async () => {
  const gate = Promise.withResolvers<{ default: () => JSX.Element }>();
  const Slow = lazy(() => gate.promise);
  const history = memoryHistory("/");
  const App = createRouter({
    history,
    routes: defineRoutes([
      defineRoute({ path: "/", component: Home }),
      defineRoute({ path: "/slow", component: Slow }),
    ]),
  });
  let routing!: () => boolean;
  mount(() => (
    <App>
      {(props) => {
        routing = useIsRouting();
        return props.children;
      }}
    </App>
  ));
  flush();
  expect(routing!()).toBe(false);
  history.set({ value: "/slow" });
  flush();
  expect(routing!()).toBe(true);
  gate.resolve({ default: () => <p>slow</p> });
  await settle();
  expect(routing!()).toBe(false);
});

test("usePreloadRoute warms code and data without navigating", async () => {
  let code = 0;
  let data = 0;
  const gate = Promise.withResolvers<{ default: () => JSX.Element }>();
  const Warmed = lazy(() => {
    code++;
    return gate.promise;
  });
  const history = memoryHistory("/");
  const App = createRouter({
    history,
    routes: defineRoutes([
      defineRoute({ path: "/", component: Home }),
      defineRoute({
        path: "/warm",
        component: Warmed,
        preload: () => void data++,
      }),
    ]),
  });
  let preload!: ReturnType<typeof usePreloadRoute>;
  mount(() => (
    <App>
      {() => {
        preload = usePreloadRoute();
        return <p>home</p>;
      }}
    </App>
  ));
  flush();
  preload("/warm", { preloadData: true });
  await settle();
  expect(code).toBe(1);
  expect(data).toBe(1);
  expect(history.get()).toBe("/");
});

test("useBeforeLeave blocks and retries navigation", async () => {
  const history = memoryHistory("/");
  const App = createRouter({
    history,
    routes: defineRoutes([
      defineRoute({
        path: "/",
        component: () => {
          useBeforeLeave((event) => {
            if (!event.defaultPrevented && event.to === "/away") event.preventDefault();
          });
          return <p>home</p>;
        },
      }),
      defineRoute({ path: "/away", component: () => <p>away</p> }),
    ]),
  });
  let navigate!: ReturnType<typeof useNavigate>;
  const { el } = mount(() => (
    <App>
      {(props) => {
        navigate = useNavigate();
        return props.children;
      }}
    </App>
  ));
  navigate("/away");
  await settle();
  expect(el.innerHTML).toBe("<p>home</p>");
  navigate("/away", { replace: true });
  await settle();
  expect(el.innerHTML).toBe("<p>home</p>");
});

test("before-leave retry forces the blocked navigation", async () => {
  const history = memoryHistory("/");
  let retry!: () => void;
  const App = createRouter({
    history,
    routes: defineRoutes([
      defineRoute({
        path: "/",
        component: () => {
          useBeforeLeave((event) => {
            event.preventDefault();
            retry = () => event.retry(true);
          });
          return <p>home</p>;
        },
      }),
      defineRoute({ path: "/away", component: () => <p>away</p> }),
    ]),
  });
  let navigate!: ReturnType<typeof useNavigate>;
  const { el } = mount(() => (
    <App>
      {(props) => {
        navigate = useNavigate();
        return props.children;
      }}
    </App>
  ));
  navigate("/away");
  await settle();
  expect(el.innerHTML).toBe("<p>home</p>");
  retry!();
  await settle();
  expect(el.innerHTML).toBe("<p>away</p>");
});

test("typed paths build urls with params, search and hash", () => {
  const App = createRouter({
    routes: defineRoutes([
      defineRoute({ path: "/users/:id", component: Home }),
      defineRoute({ path: "/about", component: Home }),
    ]),
  });
  expect(App.paths.users("42").toString()).toBe("/users/42");
  expect(App.paths.users("42")({ q: "x" }, "top")).toBe("/users/42?q=x#top");
  expect(String(App.paths.about)).toBe("/about");
  expect(App.paths.about({ q: "x" })).toBe("/about?q=x");
  expect(App.match(App.paths.users("7")()).length).toBe(1);
});

test("base prefixes matching, paths and navigation", async () => {
  const history = memoryHistory("/docs/users/7");
  const App = createRouter({
    history,
    base: "/docs",
    routes: defineRoutes([defineRoute({ path: "/users/:id", component: User })]),
  });
  const { el } = mount(() => <App />);
  expect(el.innerHTML).toBe("<p>user 7</p>");
  expect(App.paths.users("8").toString()).toBe("/docs/users/8");
  let pathname!: string;
  const Probed = createRouter({
    history,
    base: "/docs",
    routes: defineRoutes([
      defineRoute({
        path: "/users/:id",
        component: () => {
          pathname = useLocation().pathname;
          return <p>probed</p>;
        },
      }),
    ]),
  });
  mount(() => <Probed />);
  flush();
  expect(pathname!).toBe("/docs/users/7");
});

test("transformUrl rewrites before matching", () => {
  const App = createRouter({
    transformUrl: (url) => url.replace(/^\/legacy/, ""),
    routes: defineRoutes([defineRoute({ path: "/users", component: Home })]),
  });
  expect(App.match("/legacy/users")).toHaveLength(1);
  expect(App.match("/users")).toHaveLength(1);
});

test("lazy children resolve into the match chain", async () => {
  const history = memoryHistory("/feature/deep");
  const App = createRouter({
    history,
    routes: defineRoutes([
      defineRoute({
        path: "/feature",
        component: (props: RouteSectionProps) => <section>{props.children}</section>,
        children: () =>
          Promise.resolve([defineRoute({ path: "deep", component: () => <p>deep</p> })]),
      }),
    ]),
  });
  const { el } = mount(() => <App />);
  await settle(5);
  expect(el.innerHTML).toBe("<section><p>deep</p></section>");
});

test("the server renders the route for url", () => {
  const { App } = app();
  const html = renderToString(() => <App url="/users/9" />);
  expect(html).toContain("users");
  expect(html).toContain("user 9");
});

test("the instance carries routes and config", () => {
  const config = { routes };
  const router = createRouter(config);
  expect(router.routes).toBe(routes);
  expect(router.config).toBe(config);
  expect(defineRoutes(routes)).toBe(routes);
  const route = { path: "/", component: Home };
  expect(defineRoute(route)).toBe(route);
});
