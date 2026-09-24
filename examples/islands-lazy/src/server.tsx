import { renderToString } from "reze-js";

import { Page } from "./Page";

export function renderPage(): string {
  return renderToString(() => <Page />);
}
