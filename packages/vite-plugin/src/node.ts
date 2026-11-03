import type { IncomingMessage, ServerResponse } from "node:http";
import { Readable } from "node:stream";

export type Handler = (request: Request) => Promise<Response>;

function toRequest(message: IncomingMessage): Request {
  const headers = new Headers();
  for (const [name, value] of Object.entries(message.headers)) {
    if (name.startsWith(":")) continue;
    for (const item of Array.isArray(value) ? value : value ? [value] : []) headers.append(name, item);
  }
  const body = message.method === "GET" || message.method === "HEAD" ? undefined : (Readable.toWeb(message) as ReadableStream);
  return new Request(new URL(message.url ?? "/", `http://${message.headers.host ?? "localhost"}`), {
    method: message.method,
    headers,
    body,
    duplex: "half",
  } as RequestInit);
}

async function send(response: Response, out: ServerResponse): Promise<void> {
  const headers: Record<string, string | string[]> = {};
  response.headers.forEach((value, name) => {
    if (name !== "set-cookie") headers[name] = value;
  });
  const cookies = response.headers.getSetCookie();
  if (cookies.length > 0) headers["set-cookie"] = cookies;
  out.writeHead(response.status, headers);
  if (!response.body) return void out.end();
  for await (const chunk of response.body as unknown as AsyncIterable<Uint8Array>) out.write(chunk);
  out.end();
}

/** A Connect middleware that answers every request with the handler `load` resolves. */
export function middleware(load: () => Promise<Handler>) {
  return (message: IncomingMessage, out: ServerResponse, next: (error?: unknown) => void): void => {
    Promise.resolve()
      .then(load)
      .then((handler) => handler(toRequest(message)))
      .then((response) => send(response, out))
      .catch(next);
  };
}
