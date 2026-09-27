import type { JSX } from "@rezejs/dom";

import type { Href } from "./types";

type Loose = any;

/** `<a>` attributes the router reads on click: `replace`, `noscroll`, and `state` (passed as the raw string). */
export interface AnchorAttributes {
  href?: Href;
  replace?: boolean;
  noscroll?: boolean;
  state?: string;
  children?: JSX.Element;
  ref?: HTMLAnchorElement | ((el: HTMLAnchorElement) => void);
  [attribute: string]: Loose;
}

declare module "@rezejs/dom" {
  namespace JSX {
    interface IntrinsicElements {
      a: AnchorAttributes;
    }
  }
}
