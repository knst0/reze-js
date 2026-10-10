import type { RouterServerContextArgs } from "@rezejs/router";
import type { JSX } from "reze-js";

import { routes } from "./routes";

export { routes };

export function createContext({ request }: RouterServerContextArgs) {
  return { requestId: request?.headers.get("x-query-scope") ?? "missing" };
}

export default function Shell(props: { children: JSX.Element }) {
  return (
    <>
      <nav>
        <a id="nav-home" href="/">
          home
        </a>{" "}
        <a id="nav-about" href="/about">
          about
        </a>{" "}
        <a id="nav-news" href="/news">
          news
        </a>
      </nav>
      <main>{props.children}</main>
      <footer id="version">{__RZ_TEST_VERSION__}</footer>
    </>
  );
}
