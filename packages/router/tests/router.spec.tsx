import { cleanup, fire, mount, settle, tick } from "@rezejs/testing-library";
import { catchError, type JSX } from "reze-js";
import { afterEach, expect, test, vi } from "vitest";

import {
  createBrowserHistory,
  createMemoryHistory,
  createRouter,
  useBeforeLeave,
  useIsRouting,
  useNavigate,
  useSearchParams,
  type BeforeLeaveEvent,
  type HistoryEntry,
  type Navigate,
  type RouteDefinition,
  type RouterHistory,
  type RouteModule,
  type RouteProps,
} from "../src";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  sessionStorage.clear();
  window.history.replaceState(null, "", "/");
});

interface Controls {
  navigate: Navigate;
  isRouting: () => boolean;
}

interface SetupOptions {
  history?: RouterHistory;
  links?: boolean;
}

function setup(
  routes: RouteDefinition[],
  initial = "/",
  view: (children: JSX.Element) => JSX.Element = (c) => c,
  options: SetupOptions = {},
) {
  const history = options.history ?? createMemoryHistory(initial);
  const controls = {} as Controls;
  function Root(props: { children: JSX.Element }) {
    controls.navigate = useNavigate();
    controls.isRouting = useIsRouting();
    return <main>{view(props.children)}</main>;
  }
  const Router = createRouter({ routes, history, links: options.links });
  const { el } = mount(() => <Router root={Root} />);
  return { el: el.firstElementChild!, history, ...controls };
}

const Home = () => <p>home</p>;
const About = () => <p>about</p>;

test("nested routes render inside their layout and keep DOM across param changes", () => {
  const Layout = (props: RouteProps) => <section>layout({props.children})</section>;
  const Post = (props: RouteProps<{ id: string }>) => <p>post {props.params.id}</p>;
  const { el, navigate } = setup([{ path: "/blog", component: Layout, children: [{ path: "/:id", component: Post }] }], "/blog/1");
  expect(el.textContent).toBe("layout(post 1)");
  const post = el.querySelector("p");
  navigate("/blog/2");
  tick();
  expect(el.textContent).toBe("layout(post 2)");
  expect(el.querySelector("p")).toBe(post);
});

test("lazy routes keep the old view and report routing until loaded", async () => {
  const module = Promise.withResolvers<RouteModule>();
  const { el, navigate, isRouting } = setup([
    { path: "/", component: Home },
    { path: "/about", load: () => module.promise },
  ]);
  expect(isRouting()).toBe(false);
  navigate("/about");
  await settle();
  expect(el.textContent).toBe("home");
  expect(isRouting()).toBe(true);
  module.resolve({ default: About });
  await settle();
  expect(el.textContent).toBe("about");
  expect(isRouting()).toBe(false);
});

test("a navigation superseded while loading never commits", async () => {
  const module = Promise.withResolvers<RouteModule>();
  const { el, navigate } = setup([
    { path: "/", component: Home },
    { path: "/slow", load: () => module.promise },
    { path: "/about", component: About },
  ]);
  navigate("/slow");
  navigate("/about");
  tick();
  module.resolve({ default: () => <p>slow</p> });
  await settle();
  expect(el.textContent).toBe("about");
});

test("preload receives params and intent, and its result is the route's data", () => {
  const preload = vi.fn(({ params }: { params: { id: string } }) => "data " + params.id);
  const { el, navigate } = setup([
    { path: "/", component: Home },
    { path: "/p/:id", preload, component: (props: RouteProps<{ id: string }, string>) => <i>{props.data}</i> },
  ]);
  navigate("/p/3");
  tick();
  expect(preload).toHaveBeenCalledWith(expect.objectContaining({ params: { id: "3" }, intent: "navigate" }));
  expect(el.textContent).toBe("data 3");
});

test("same-origin anchor clicks navigate; modified, targeted, download, external and unrouted links do not", () => {
  const seen: boolean[] = [];
  const record = (event: Event): void => {
    seen.push(event.defaultPrevented);
    event.preventDefault();
  };
  window.addEventListener("click", record);
  try {
    const { el, history } = setup(
      [
        { path: "/", component: Home },
        { path: "/about", component: About },
      ],
      "/",
      (children) => (
        <>
          <a id="plain" href="/about">
            a
          </a>
          <a id="blank" href="/about" target="_blank">
            b
          </a>
          <a id="download" href="/about" download>
            c
          </a>
          <a id="external" href="https://other.test/x">
            d
          </a>
          <a id="unrouted" href="/files/report.csv">
            e
          </a>
          {children}
        </>
      ),
      { links: true },
    );
    const link = (id: string) => el.querySelector(`#${id}`)!;
    fire(link("plain"), "click", { ctrlKey: true } as MouseEventInit);
    fire(link("blank"), "click");
    fire(link("download"), "click");
    fire(link("external"), "click");
    fire(link("unrouted"), "click");
    expect(seen).toEqual([false, false, false, false, false]);
    expect(history.get().path).toBe("/");
    fire(link("plain"), "click");
    tick();
    expect(seen.at(-1)).toBe(true);
    expect(history.get().path).toBe("/about");
    expect(el.textContent).toContain("about");
  } finally {
    window.removeEventListener("click", record);
  }
});

test("leave guards prevent navigation, retry completes it, and prevented history moves are undone", () => {
  let isBlocking = true;
  let prevented: BeforeLeaveEvent | undefined;
  const {
    el,
    navigate,
    history: memory,
  } = setup(
    [
      { path: "/", component: Home },
      { path: "/about", component: About },
    ],
    "/",
    (children) => {
      useBeforeLeave((event) => {
        if (!isBlocking) return;
        event.preventDefault();
        prevented = event;
      });
      return children;
    },
  );
  navigate("/about");
  tick();
  expect(el.textContent).toBe("home");
  expect(prevented?.to).toBe("/about");
  prevented!.retry(true);
  tick();
  expect(el.textContent).toBe("about");
  expect(memory.get().path).toBe("/about");
  navigate(-1);
  tick();
  expect(prevented?.to).toBe(-1);
  expect(memory.get().path).toBe("/about");
  expect(el.textContent).toBe("about");
  isBlocking = false;
  navigate(-1);
  tick();
  expect(el.textContent).toBe("home");
});

test("the search params setter merges into the query and deletes nullish keys", () => {
  let setQuery!: ReturnType<typeof useSearchParams>[1];
  const { history } = setup([{ path: "/s", component: Home }], "/s?page=2", (children) => {
    setQuery = useSearchParams()[1];
    return children;
  });
  setQuery({ q: "a", page: null });
  expect(history.get().path).toBe("/s?q=a");
});

test("a failed load reaches catchError and the next navigation retries it", async () => {
  const error = new Error("offline");
  const load = vi.fn<() => Promise<RouteModule>>().mockRejectedValueOnce(error).mockResolvedValueOnce({ default: About });
  const handler = vi.fn();
  const history = createMemoryHistory("/");
  let navigate!: Navigate;
  const Root = (props: { children: JSX.Element }) => {
    navigate = useNavigate();
    return <main>{props.children}</main>;
  };
  const routes: RouteDefinition[] = [
    { path: "/", component: Home },
    { path: "/about", load },
  ];
  const Router = createRouter({ routes, history });
  const { el } = mount(() => catchError(() => <Router root={Root} />, handler));
  navigate("/about");
  await settle();
  expect(handler).toHaveBeenCalledWith(error);
  navigate("/");
  tick();
  navigate("/about");
  await settle();
  expect(load).toHaveBeenCalledTimes(2);
  expect(el.textContent).toBe("about");
});

test("back/forward restores the scroll position saved for that entry and no other", () => {
  let entry: HistoryEntry = { path: "/", state: undefined, index: 0 };
  let onPop!: (entry: HistoryEntry) => void;
  const history: RouterHistory = {
    get: () => entry,
    push: (path, state) => void (entry = { path, state, index: entry.index + 1 }),
    replace: (path, state) => void (entry = { path, state, index: entry.index }),
    go: () => {},
    listen: (listener) => ((onPop = listener), () => {}),
    resolve: (url) => url.pathname,
    base: "",
    scroll: true,
  };
  const pop = (path: string, index: number): void => {
    entry = { path, state: undefined, index };
    onPop(entry);
  };
  const scrollTo = vi.spyOn(window, "scrollTo").mockImplementation(() => {});
  let navigate!: Navigate;
  const Root = (props: { children: JSX.Element }) => {
    navigate = useNavigate();
    return <main>{props.children}</main>;
  };
  const Router = createRouter({
    routes: [
      { path: "/", component: Home },
      { path: "/about", component: About },
    ],
    history,
  });
  mount(() => <Router root={Root} />);
  vi.stubGlobal("scrollY", 300);
  navigate("/about");
  vi.stubGlobal("scrollY", 0);
  pop("/", 0);
  expect(scrollTo).toHaveBeenLastCalledWith(0, 300);
  pop("/about", 128);
  expect(scrollTo).toHaveBeenLastCalledWith(0, 0);
  vi.unstubAllGlobals();
  scrollTo.mockRestore();
});

const pages: RouteDefinition[] = [
  { path: "/", component: Home },
  { path: "/about", component: About },
];

function silenceScroll(): ReturnType<typeof vi.spyOn> {
  return vi.spyOn(window, "scrollTo").mockImplementation(() => {});
}

test("navigate takes route paths while anchors carry the base", () => {
  silenceScroll();
  window.history.replaceState(null, "", "/app/");
  const { el, navigate } = setup(
    pages,
    "/",
    (children) => (
      <>
        {children}
        <a href="/app/">home</a>
      </>
    ),
    {
      history: createBrowserHistory("/app/"),
    },
  );
  navigate("/about");
  tick();
  expect(location.pathname).toBe("/app/about");
  expect(el.textContent).toContain("about");
  fire(el.querySelector("a")!, "click");
  tick();
  expect(location.pathname).toBe("/app/");
  expect(el.textContent).toContain("home");
});

test("navigate routes same-origin absolute URLs", () => {
  const { history, navigate } = setup(pages);
  navigate(`${location.origin}/about?x=1`);
  expect(history.get().path).toBe("/about?x=1");
});

test.runIf(navigator.userAgent.includes("HappyDOM"))(
  "navigate hands absolute URLs outside the router to the browser and never runs scripts",
  () => {
    const assign = vi.spyOn(location, "assign").mockImplementation(() => {});
    const { history, navigate } = setup(pages);
    navigate("https://other.test/about");
    expect(assign).toHaveBeenCalledWith("https://other.test/about");
    expect(history.get().path).toBe("/");
    navigate(" JavaScript:alert(1)");
    navigate("java\tscript:alert(1)");
    expect(assign).toHaveBeenCalledOnce();
  },
);

test("a malformed hash target is looked up raw instead of throwing", () => {
  silenceScroll();
  const { el } = setup(
    [{ path: "/", component: () => <h2 id="%E0%A4%A">t</h2> }],
    "/",
    (children) => (
      <>
        {children}
        <a href="#%E0%A4%A">bad</a>
      </>
    ),
    { history: createBrowserHistory() },
  );
  const scrollIntoView = vi.fn();
  el.querySelector("h2")!.scrollIntoView = scrollIntoView;
  fire(el.querySelector("a")!, "click");
  expect(scrollIntoView).toHaveBeenCalledOnce();
});

test("a memory router leaves the document's anchors to the browser by default", () => {
  let isPrevented: boolean | undefined;
  window.addEventListener(
    "click",
    (event) => {
      isPrevented = event.defaultPrevented;
      event.preventDefault();
    },
    { once: true },
  );
  const { el, history, navigate } = setup(pages, "/", (children) => (
    <>
      {children}
      <a href="/about">a</a>
    </>
  ));
  fire(el.querySelector("a")!, "click");
  expect(isPrevented).toBe(false);
  expect(history.get().path).toBe("/");
  navigate("/about");
  tick();
  expect(el.textContent).toContain("about");
});

test("scroll positions outlive the document through sessionStorage", () => {
  const scrollTo = silenceScroll();
  window.history.replaceState(null, "", "/about");
  setup(pages, "/", (children) => children, { history: createBrowserHistory() });
  expect(scrollTo).not.toHaveBeenCalled();
  vi.stubGlobal("scrollY", 420);
  window.dispatchEvent(new Event("pagehide"));
  vi.stubGlobal("scrollY", 0);
  cleanup();
  setup(pages, "/", (children) => children, { history: createBrowserHistory() });
  expect(scrollTo).toHaveBeenLastCalledWith(0, 420);
});

test("navigating while route modules load builds on the pending location", async () => {
  const module = Promise.withResolvers<RouteModule>();
  let setQuery!: ReturnType<typeof useSearchParams>[1];
  const { el, history, navigate } = setup(
    [
      { path: "/", component: Home },
      { path: "/slow", load: () => module.promise },
    ],
    "/",
    (children) => {
      setQuery = useSearchParams()[1];
      return children;
    },
  );
  navigate("/slow?x=1");
  setQuery({ y: 2 });
  expect(history.get().path).toBe("/slow?x=1&y=2");
  navigate("#top");
  expect(history.get().path).toBe("/slow?x=1&y=2#top");
  module.resolve({ default: About });
  await settle();
  expect(el.textContent).toBe("about");
});

test("a throwing leave guard is reported without blocking, and guards also cover unloading the document", () => {
  const reportError = vi.fn();
  vi.stubGlobal("reportError", reportError);
  let isBlocking = false;
  const { el, navigate } = setup(pages, "/", (children) => {
    useBeforeLeave(() => {
      throw new Error("bug");
    });
    useBeforeLeave((event) => {
      if (isBlocking) event.preventDefault();
    });
    return children;
  });
  navigate("/about");
  tick();
  expect(reportError).toHaveBeenCalledWith(new Error("bug"));
  expect(el.textContent).toBe("about");
  isBlocking = true;
  const unload = new Event("beforeunload", { cancelable: true });
  window.dispatchEvent(unload);
  expect(unload.defaultPrevented).toBe(true);
  cleanup();
  const afterDispose = new Event("beforeunload", { cancelable: true });
  window.dispatchEvent(afterDispose);
  expect(afterDispose.defaultPrevented).toBe(false);
});

test("repeated query keys keep every value and the setter writes arrays in place of the key", () => {
  let search!: ReturnType<typeof useSearchParams>;
  const { history } = setup([{ path: "/s", component: Home }], "/s?tag=a&q=x&tag=b", (children) => {
    search = useSearchParams();
    return children;
  });
  expect({ ...search[0]() }).toEqual({ tag: ["a", "b"], q: "x" });
  search[1]({ tag: ["c", null, "d"], q: "y" });
  expect(history.get().path).toBe("/s?q=y&tag=c&tag=d");
});

test("a hash-only change keeps route data without re-running preload", () => {
  const preload = vi.fn(() => "data");
  const { navigate } = setup([{ path: "/", component: Home, preload }]);
  navigate("#a");
  navigate("/#b");
  expect(preload).toHaveBeenCalledOnce();
  navigate("/?q=1");
  expect(preload).toHaveBeenCalledTimes(2);
});

test("focus preloads a link's route, retries after a failed load, and swallows preload rejections", async () => {
  const load = vi
    .fn<() => Promise<RouteModule>>()
    .mockRejectedValueOnce(new Error("offline"))
    .mockResolvedValue({ default: About, route: { preload: () => Promise.reject(new Error("no data")) } });
  const { el } = setup(
    [
      { path: "/", component: Home },
      { path: "/about", load },
    ],
    "/",
    (children) => (
      <>
        {children}
        <a href="/about">a</a>
      </>
    ),
    { links: true },
  );
  const anchor = el.querySelector("a")!;
  fire(anchor, "focusin");
  await settle();
  fire(anchor, "focusin");
  await settle();
  expect(load).toHaveBeenCalledTimes(2);
  fire(anchor, "focusin");
  await settle();
  expect(load).toHaveBeenCalledTimes(2);
});

test("a redirect from preload wins over the navigation that ran it", () => {
  let navigate!: Navigate;
  const routes: RouteDefinition[] = [
    ...pages,
    { path: "/admin", component: () => <p>secret</p>, preload: () => navigate("/about", { replace: true }) },
  ];
  const result = setup(routes);
  navigate = result.navigate;
  navigate("/admin");
  tick();
  expect(result.history.get().path).toBe("/about");
  expect(result.el.textContent).toBe("about");
});

test.each(["/admin", "/"])("a redirect from a route component renders its target (starting at %s)", async (initial) => {
  const Admin = () => {
    useNavigate()("/about", { replace: true });
    return <p>secret</p>;
  };
  const { el, history, navigate } = setup([...pages, { path: "/admin", component: Admin }], initial);
  if (initial !== "/admin") navigate("/admin");
  await settle();
  expect(history.get().path).toBe("/about");
  expect(el.textContent).toBe("about");
});
