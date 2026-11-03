import type { JSX } from "reze-js";

export default function Root(props: { children: JSX.Element }) {
  return (
    <>
      <head>
        <title>Counter</title>
      </head>
      {props.children}
    </>
  );
}
