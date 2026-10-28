import type { PreloadArgs } from "@rezejs/router";
import type { JSX } from "reze-js";

export const route = {
  beforeLoad: ({ location, context }: PreloadArgs<{}, { tenant?: string } | undefined>) => ({
    requestId: `${context?.tenant ?? "none"}:${String(location.query.scope ?? "missing")}`,
  }),
};

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
      <details id="layout-details" open>
        <summary>layout state</summary>
        <p>kept across pages</p>
      </details>
      <main>{props.children}</main>
      <footer id="version">{__RZ_TEST_VERSION__}</footer>
    </>
  );
}
