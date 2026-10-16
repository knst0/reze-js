import type { JSX } from "reze-js";

export default function Document(props: { children: JSX.Element }) {
  return (
    <html lang="en">
      <head>
        <meta charset="UTF-8" />
        <title>Module state</title>
      </head>
      <body>
        <div id="app">{props.children}</div>
      </body>
    </html>
  );
}
