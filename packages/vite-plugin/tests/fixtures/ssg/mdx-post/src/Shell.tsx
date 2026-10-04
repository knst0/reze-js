import type { JSX } from "reze-js";

export function Shell(props: { children: JSX.Element }) {
  return <main>{props.children}</main>;
}
