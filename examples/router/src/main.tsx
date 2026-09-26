import {
  createRouter,
  defineRoute,
  defineRoutes,
  useIsRouting,
  useLocation,
  type RouteSectionProps,
} from "@rezejs/router";
import { lazy, Loading, render } from "reze-js";

const User = lazy(() => import("./User"));

function Layout(props: RouteSectionProps) {
  const isRouting = useIsRouting();
  return (
    <>
      <nav>
        <a href="/">Home</a> <a href="/users">Users</a> <a href="/nowhere">Broken link</a>
        {isRouting() && <span> loading…</span>}
      </nav>
      <main>{props.children}</main>
    </>
  );
}

function Home() {
  return <h1>Home</h1>;
}

function Users(props: RouteSectionProps) {
  return (
    <>
      <h1>Users</h1>
      <ul>
        <li>
          <a href="/users/1">Ada</a>
        </li>
        <li>
          <a href="/users/2">Grace</a>
        </li>
      </ul>
      {props.children}
    </>
  );
}

function NotFound() {
  return <p>Nothing at {useLocation().pathname}.</p>;
}

const Router = createRouter({
  routes: defineRoutes([
    defineRoute({
      path: "/",
      component: Layout,
      children: [
        defineRoute({ path: "/", component: Home }),
        defineRoute({
          path: "users",
          component: Users,
          children: [
            defineRoute({ path: "/", component: () => null }),
            defineRoute({ path: ":id", component: User }),
          ],
        }),
        defineRoute({ path: "*rest", component: NotFound }),
      ],
    }),
  ]),
});

render(
  () => (
    <Loading fallback={<p>loading…</p>}>
      <Router />
    </Loading>
  ),
  document.getElementById("app")!,
);
