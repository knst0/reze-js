import { DefaultPort, guardedFetch } from "./shared";
import type { RequestHandler, RunningServer, ServeOptions } from "./shared";

export { createNodeListener } from "./node-listener";

declare const Bun: {
  serve(options: { port: number; hostname?: string; fetch: (request: Request) => Promise<Response> }): {
    readonly hostname: string;
    readonly port: number;
    stop(closeActiveConnections?: boolean): Promise<void>;
  };
};

export async function serve(handler: RequestHandler, options: ServeOptions = {}): Promise<RunningServer> {
  const server = Bun.serve({
    port: options.port ?? DefaultPort,
    hostname: options.hostname,
    fetch: (request) => guardedFetch(handler, request),
  });
  return {
    hostname: server.hostname,
    port: server.port,
    close: () => server.stop(),
  };
}
