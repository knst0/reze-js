import { useLocation, type PreloadArgs } from "@rezejs/router";
import type { JSX } from "reze-js";

import "../styles.css";

function contextId(pathname: string): number {
  let id = 0;
  for (const char of pathname) id = (id * 31 + char.charCodeAt(0)) >>> 0;
  return id;
}

export const route = {
  beforeLoad: ({ location }: PreloadArgs) => ({ id: contextId(location.pathname), pathname: location.pathname }),
};

export default function Shell(props: { children: JSX.Element }) {
  const location = useLocation();
  return (
    <>
      <nav>
        <a href="/">home</a> <a href="/about">about</a> <a href="/docs/intro">intro</a>
      </nav>
      <p id="path">{location().pathname}</p>
      <main>{props.children}</main>
    </>
  );
}
