import type { JSX } from "reze-js";

import markUrl from "./mark.svg?url";

export default function Document(props: { children: JSX.Element }) {
  return (
    <html lang="en">
      <head>
        <meta charset="UTF-8" />
        <title>Context</title>
        <link rel="preload" as="image" type="image/svg+xml" href={markUrl} />
      </head>
      <body>
        <div id="app">{props.children}</div>
      </body>
    </html>
  );
}
