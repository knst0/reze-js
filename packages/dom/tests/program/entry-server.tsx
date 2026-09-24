import { renderToString, ssrIsland } from "@rezejs/dom";

import { Counter } from "./Counter";
import { Page } from "./Page";

export function renderPage(): string {
  return renderToString(() => <Page />);
}

export function renderFn(): string {
  return renderToString(
    () => ssrIsland("e2e", Counter, { start: 1, label: "fn", run: () => 1 }),
    true,
  );
}
