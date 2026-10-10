import { useLocation, type RouterServerContextArgs } from "@rezejs/router";
import type { JSX } from "reze-js";

import { routes } from "./routes";

export { routes };

let nextContextId = 0;

export function createContext({ pathname, mode }: RouterServerContextArgs) {
  return { id: ++nextContextId, pathname, mode };
}

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
