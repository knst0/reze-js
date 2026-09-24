import { Title } from "@rezejs/meta";
import { Router, useIsRouting, useMatch } from "@rezejs/router";
import { Loading, render } from "reze-js";
import type { JSX } from "reze-js/jsx-runtime";
import routes from "virtual:reze-routes";

function NavLink(props: { href: string; children: JSX.Element }) {
  const match = useMatch(() => props.href);
  return (
    <a href={props.href} aria-current={match() ? "page" : undefined}>
      {props.children}
    </a>
  );
}

function Layout(props: { children: JSX.Element }) {
  const isRouting = useIsRouting();
  return (
    <>
      <Title>Reze · Router</Title>
      <nav>
        <NavLink href="/">Home</NavLink> <NavLink href="/users">Users</NavLink>{" "}
        <a href="/nowhere">Broken link</a>
        {isRouting() && <span> loading…</span>}
      </nav>
      <main>{props.children}</main>
    </>
  );
}

render(
  () => (
    <Loading fallback={<p>loading…</p>}>
      <Router routes={routes} root={Layout} />
    </Loading>
  ),
  document.getElementById("app")!,
);
