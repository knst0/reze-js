import type { JSX } from "reze-js";

export default function Document(props: { children: JSX.Element }) {
  return (
    <html lang="en">
      <head>
        <meta charset="UTF-8" />
        <meta name="viewport" content="width=device-width, initial-scale=1.0" />
        <title>Portals and islands</title>
      </head>
      <body>
        <div id="app">{props.children}</div>
      </body>
    </html>
  );
}
