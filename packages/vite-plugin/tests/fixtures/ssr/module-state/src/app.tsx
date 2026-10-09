import type { JSX } from "reze-js";

import { routes } from "./routes";

export { routes };

export default function Shell(props: { children: JSX.Element }) {
  return <main>{props.children}</main>;
}
