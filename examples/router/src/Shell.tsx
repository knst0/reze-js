import { useIsRouting } from "@rezejs/router";
import type { JSX } from "reze-js";
import { paths } from "virtual:reze-routes";

export function Shell(props: { children: JSX.Element }) {
  const isRouting = useIsRouting();
  return (
    <>
      <nav>
        <a href={paths.index()}>Home</a> <a href={paths.about()}>About</a> <a href={paths.blog()}>Blog</a> <a href={paths.login()}>Login</a>
        {isRouting() ? <span id="routing"> loading…</span> : null}
      </nav>
      <main>{props.children}</main>
    </>
  );
}
