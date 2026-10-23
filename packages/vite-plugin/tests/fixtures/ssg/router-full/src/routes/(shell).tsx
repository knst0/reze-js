import { useLocation, type PreloadArgs } from "@rezejs/router";
import type { JSX } from "reze-js";

import "../styles.css";

let nextContextId = 0;

export const route = {
  beforeLoad: ({ location }: PreloadArgs) => ({ id: ++nextContextId, pathname: location.pathname }),
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
