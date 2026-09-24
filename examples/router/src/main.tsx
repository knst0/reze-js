import { A, Outlet, Route, Router, useIsRouting, useLocation } from "@rezejs/router";
import { lazy, Loading, render } from "reze-js";

const User = lazy(() => import("./User"));

function Layout() {
  const isRouting = useIsRouting();
  return (
    <>
      <nav>
        <A href="/">Home</A> <A href="/users">Users</A> <A href="/nowhere">Broken link</A>
        {isRouting() && <span> loading…</span>}
      </nav>
      <main>
        <Outlet />
      </main>
    </>
  );
}

function Home() {
  return <h1>Home</h1>;
}

function Users() {
  return (
    <>
      <h1>Users</h1>
      <ul>
        <li>
          <A href="/users/1">Ada</A>
        </li>
        <li>
          <A href="/users/2">Grace</A>
        </li>
      </ul>
      <Outlet />
    </>
  );
}

function NotFound() {
  return <p>Nothing at {useLocation().pathname}.</p>;
}

render(
  () => (
    <Loading fallback={<p>loading…</p>}>
      <Router>
        <Route path="/" component={Layout}>
          <Route path="/" component={Home} />
          <Route path="users" component={Users}>
            <Route path=":id" component={User} />
          </Route>
          <Route path="*rest" component={NotFound} />
        </Route>
      </Router>
    </Loading>
  ),
  document.getElementById("app")!,
);
