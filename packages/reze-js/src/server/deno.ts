import { DefaultPort, guardedFetch } from "./shared";
import type { RequestHandler, RunningServer, ServeOptions } from "./shared";

export { createNodeListener } from "./node-listener";

declare const Deno: {
  serve(
    options: { port: number; hostname?: string; onListen(): void },
    handler: (request: Request) => Promise<Response>,
  ): {
    readonly addr: { readonly hostname: string; readonly port: number };
    shutdown(): Promise<void>;
  };
};

export async function serve(handler: RequestHandler, options: ServeOptions = {}): Promise<RunningServer> {
  const server = Deno.serve({ port: options.port ?? DefaultPort, hostname: options.hostname, onListen() {} }, (request) =>
    guardedFetch(handler, request),
  );
  return {
    hostname: server.addr.hostname,
    port: server.addr.port,
    close: () => server.shutdown(),
  };
}
