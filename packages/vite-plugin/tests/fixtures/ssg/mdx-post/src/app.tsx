import type { JSX } from "reze-js";

import "./styles.css";

export function Shell(props: { children: JSX.Element }) {
  return <main>{props.children}</main>;
}

export default Shell;
