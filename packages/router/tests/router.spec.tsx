import { lazy, Loading, renderToString } from "@rezejs/dom";
import type { JSX } from "@rezejs/dom/jsx-runtime";
import { flush } from "@rezejs/signals";
import { cleanup, fire, mount } from "@rezejs/test-utils";
import { afterEach, beforeEach, expect, test } from "vitest";

import {
  A,
  compilePattern,
  matchPattern,
  Outlet,
  Route,
  Router,
  useLocation,
  useNavigate,
  useParams,
} from "../src";

afterEach(cleanup);
beforeEach(() => window.history.replaceState(null, "", "/"));

async function settle(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 0));
  flush();
}

test("patterns bind params, optional segments and the rest", () => {
  expect(matchPattern(compilePattern("/users/:id"), "/users/7")).toEqual({ id: "7" });
  expect(matchPattern(compilePattern("/users/:id"), "/users")).toBeUndefined();
  expect(matchPattern(compilePattern("/posts/:page?"), "/posts")).toEqual({});
  expect(matchPattern(compilePattern("/files/*path"), "/files/a/b%20c")).toEqual({ path: "a/b c" });
  expect(compilePattern("/a/b").score).toBeGreaterThan(compilePattern("/a/:x").score);
  expect(compilePattern("/a/:x").score).toBeGreaterThan(compilePattern("/a/*rest").score);
  expect(compilePattern("/").score).toBeGreaterThan(compilePattern("/*rest").score);
});

const Home = () => <h1>home</h1>;
const Users = () => (
  <section>
    <h1>users</h1>
    <Outlet />
  </section>
);
const User = () => {
  const params = useParams();
  return <p>user {params.id}</p>;
};
const NotFound = () => <p>missing {useLocation().pathname}</p>;

function App(props: { url?: string }): JSX.Element {
  return (
    <Router url={props.url}>
      <Route path="/" component={Home} />
      <Route path="/users" component={Users}>
        <Route path=":id" component={User} />
      </Route>
      <Route path="/users/new" component={() => <p>new user</p>} />
      <Route path="*rest" component={NotFound} />
    </Router>
  );
}

test("the most specific route wins and nested routes render in the outlet", () => {
  window.history.replaceState(null, "", "/users/7");
  const { el } = mount(() => <App />);
  expect(el.innerHTML).toBe("<section><h1>users</h1><p>user 7</p></section>");
  cleanup();
  window.history.replaceState(null, "", "/users/new");
  expect(mount(() => <App />).el.innerHTML).toBe("<p>new user</p>");
  cleanup();
  window.history.replaceState(null, "", "/nope/deep");
  expect(mount(() => <App />).el.innerHTML).toBe("<p>missing /nope/deep</p>");
});

test("A navigates on a plain click, marks the current link and keeps a modified click", async () => {
  let go!: ReturnType<typeof useNavigate>;
  const Nav = () => {
    go = useNavigate();
    return (
      <nav>
        <A href="/users/1">one</A>
        <A href="/">home</A>
      </nav>
    );
  };
  const { el } = mount(() => (
    <Router>
      <Route
        path="/"
        component={() => (
          <>
            <Nav />
            <Home />
          </>
        )}
      />
      <Route
        path="/users/:id"
        component={() => (
          <>
            <Nav />
            <User />
          </>
        )}
      />
    </Router>
  ));
  const link = el.querySelector("a")!;
  expect(el.querySelectorAll("a")[1]!.getAttribute("aria-current")).toBe("page");
  const modified = new MouseEvent("click", { bubbles: true, cancelable: true, ctrlKey: true });
  link.dispatchEvent(modified);
  expect(modified.defaultPrevented).toBe(false);
  fire(link, "click");
  await settle();
  expect(window.location.pathname).toBe("/users/1");
  expect(el.querySelector("p")!.textContent).toBe("user 1");
  expect(el.querySelector("a")!.getAttribute("aria-current")).toBe("page");
  await go("2");
  flush();
  expect(el.querySelector("p")!.textContent).toBe("user 2");
});

test("back and forward follow popstate", async () => {
  let go!: ReturnType<typeof useNavigate>;
  const { el } = mount(() => (
    <Router>
      <Route
        path="/"
        component={() => {
          go = useNavigate();
          return <Home />;
        }}
      />
      <Route path="/users/:id" component={User} />
    </Router>
  ));
  await go("/users/3");
  flush();
  expect(el.textContent).toBe("user 3");
  window.history.replaceState(null, "", "/");
  window.dispatchEvent(new PopStateEvent("popstate", { state: { scroll: 0 } }));
  await settle();
  expect(el.textContent).toBe("home");
});

test("a lazy route keeps the current page inside Loading until it loaded", async () => {
  const gate = Promise.withResolvers<{ default: () => JSX.Element }>();
  const Slow = lazy(() => gate.promise);
  let go!: ReturnType<typeof useNavigate>;
  const { el } = mount(() => (
    <Loading fallback={<i>loading</i>}>
      <Router>
        <Route
          path="/"
          component={() => {
            go = useNavigate();
            return <Home />;
          }}
        />
        <Route path="/slow" component={Slow} />
      </Router>
    </Loading>
  ));
  expect(el.innerHTML).toBe("<h1>home</h1>");
  const done = go("/slow");
  await settle();
  expect(el.innerHTML).toBe("<h1>home</h1>");
  expect(window.location.pathname).toBe("/");
  gate.resolve({ default: () => <p>slow page</p> });
  await done;
  flush();
  expect(el.innerHTML).toBe("<p>slow page</p>");
  expect(window.location.pathname).toBe("/slow");
});

test("a child route with its parent's path renders inside the parent's outlet", () => {
  const Layout = () => (
    <main>
      <Outlet />
    </main>
  );
  const { el } = mount(() => (
    <Router>
      <Route path="/" component={Layout}>
        <Route path="/" component={Home} />
        <Route path="*rest" component={NotFound} />
      </Route>
    </Router>
  ));
  expect(el.innerHTML).toBe("<main><h1>home</h1></main>");
});

test("the server renders the route for url", () => {
  const html = renderToString(() => <App url="/users/9" />);
  expect(html).toContain("users");
  expect(html).toContain("user 9");
});
