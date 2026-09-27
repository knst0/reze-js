import { useIsRouting } from "@rezejs/router";
import type { JSX } from "reze-js";

export function Shell(props: { children: JSX.Element }) {
  const isRouting = useIsRouting();
  return (
    <>
      <nav>
        <a href="/">Home</a> <a href="/about">About</a> <a href="/blog">Blog</a> <a href="/login">Login</a>
        {isRouting() ? <span id="routing"> loading…</span> : null}
      </nav>
      <main>{props.children}</main>
    </>
  );
}
