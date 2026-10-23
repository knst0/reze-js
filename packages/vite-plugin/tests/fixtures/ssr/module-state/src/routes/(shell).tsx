import type { JSX } from "reze-js";

export default function Shell(props: { children: JSX.Element }) {
  return <main>{props.children}</main>;
}
