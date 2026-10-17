import { existsSync, readFileSync } from "node:fs";

import { compileDocument } from "@rezejs/compiler";

import { ClientRequest } from "./adapter";
import { withBootstrap } from "./template";

export interface AppDocument {
  html: string;
  rootId: string;
}

const DefaultDocumentSource = `export default function Document(props) {
  return (
    <html lang="en">
      <head>
        <meta charset="utf-8" />
        <meta name="viewport" content="width=device-width, initial-scale=1" />
      </head>
      <body>
        <div id="app">{props.children}</div>
      </body>
    </html>
  );
}
`;

export function loadDocument(file: string): AppDocument {
  const source = existsSync(file) ? readFileSync(file, "utf8") : DefaultDocumentSource;
  const result = compileDocument(source, file);
  if (result.html === undefined || result.rootId === undefined) {
    const rendered = result.diagnostics.map((d) => `${d.rendered.trimEnd()}\n  repair guide: ${d.docs}`);
    throw new Error(rendered.join("\n\n"));
  }
  return { html: withBootstrap(result.html, ClientRequest), rootId: result.rootId };
}
