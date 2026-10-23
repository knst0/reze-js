import type { JSX } from "reze-js";

import "../styles.css";

export default function Shell(props: { children: JSX.Element }) {
  return <main>{props.children}</main>;
}
