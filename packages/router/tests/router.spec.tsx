import { catchError, type JSX } from "reze-js";
import { afterEach, expect, test, vi } from "vitest";

import {
  createMemoryHistory,
  Router,
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
import { cleanup, fire, mount, tick } from "./utils";

afterEach(cleanup);

function settle(): Promise<void> {
  const { promise, resolve } = Promise.withResolvers<void>();
  setTimeout(resolve, 0);
  return promise;
}

interface Controls {
  navigate: Navigate;
  isRouting: () => boolean;
}

function setup(routes: RouteDefinition[], initial = "/", view: (children: JSX.Element) => JSX.Element = (c) => c) {
  const history = createMemoryHistory(initial);
  const controls = {} as Controls;
  function Root(props: { children: JSX.Element }) {
    controls.navigate = useNavigate();
    controls.isRouting = useIsRouting();
    return <main>{view(props.children)}</main>;
  }
  const { el } = mount(() => <Router routes={routes} history={history} root={Root} />);
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

test("same-origin anchor clicks navigate; modified, targeted, download and external links do not", () => {
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
          {children}
        </>
      ),
    );
    const link = (id: string) => el.querySelector(`#${id}`)!;
    fire(link("plain"), "click", { ctrlKey: true } as MouseEventInit);
    fire(link("blank"), "click");
    fire(link("download"), "click");
    fire(link("external"), "click");
    expect(seen).toEqual([false, false, false, false]);
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
  const { el } = mount(() => catchError(() => <Router routes={routes} history={history} root={Root} />, handler));
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
  mount(() => (
    <Router
      routes={[
        { path: "/", component: Home },
        { path: "/about", component: About },
      ]}
      history={history}
      root={Root}
    />
  ));
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
