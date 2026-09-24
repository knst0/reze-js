import { hydrate } from "@rezejs/dom";

import { Page } from "./Page";

export function hydratePage(el: HTMLElement): () => void {
  return hydrate(() => <Page />, el);
}
