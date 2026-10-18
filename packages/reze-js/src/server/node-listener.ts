import type { IncomingHttpHeaders, IncomingMessage, ServerResponse } from "node:http";
import { Readable } from "node:stream";
import { pipeline } from "node:stream/promises";
import type { ReadableStream as NodeReadableStream } from "node:stream/web";

import type { RequestHandler } from "./shared";

export type NodeListener = (req: IncomingMessage, res: ServerResponse) => void;

export function createNodeListener(handler: RequestHandler): NodeListener {
  return (req, res) => {
    const controller = new AbortController();
    res.once("close", () => {
      if (!res.writableFinished) controller.abort();
    });
    const method = req.method ?? "GET";
    const hasBody = method !== "GET" && method !== "HEAD";
    const request = new Request(requestUrl(req), {
      method,
      headers: nodeHeaders(req.headers),
      body: hasBody ? (Readable.toWeb(req) as ReadableStream) : undefined,
      duplex: "half",
      signal: controller.signal,
    } as RequestInit);
    handler(request).then(
      (response) => writeResponse(res, response),
      (error: unknown) => {
        console.error(error);
        if (!res.headersSent) res.statusCode = 500;
        res.end("Internal Server Error");
      },
    );
  };
}

function requestUrl(req: IncomingMessage): string {
  const protocol = "encrypted" in req.socket && req.socket.encrypted ? "https" : "http";
  // HTTP/2 (Vite dev over https) carries the host in `:authority` instead of `host`.
  const host = req.headers.host ?? req.headers[":authority"] ?? "localhost";
  return `${protocol}://${host}${req.url ?? "/"}`;
}

function nodeHeaders(source: IncomingHttpHeaders): Headers {
  const headers = new Headers();
  for (const [name, value] of Object.entries(source)) {
    // HTTP/2 pseudo-headers (`:method`, `:path`, …) are not valid Fetch header names.
    if (value === undefined || name.startsWith(":")) continue;
    for (const item of Array.isArray(value) ? value : [value]) headers.append(name, item);
  }
  return headers;
}

async function writeResponse(res: ServerResponse, response: Response): Promise<void> {
  res.statusCode = response.status;
  response.headers.forEach((value, name) => {
    if (name !== "set-cookie") res.setHeader(name, value);
  });
  const cookies = response.headers.getSetCookie();
  if (cookies.length > 0) res.setHeader("set-cookie", cookies);
  if (response.body === null) {
    res.end();
    return;
  }
  await pipeline(Readable.fromWeb(response.body as NodeReadableStream), res).catch(() => {
    res.destroy();
  });
}
