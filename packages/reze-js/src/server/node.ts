import { createServer } from "node:http";
import type { AddressInfo } from "node:net";

import { createNodeListener } from "./node-listener";
import { DefaultPort } from "./shared";
import type { RequestHandler, RunningServer, ServeOptions } from "./shared";

export { createNodeListener };

export async function serve(handler: RequestHandler, options: ServeOptions = {}): Promise<RunningServer> {
  const server = createServer(createNodeListener(handler));
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(options.port ?? DefaultPort, options.hostname, () => {
      server.off("error", reject);
      resolve();
    });
  });
  const address = server.address() as AddressInfo;
  return {
    hostname: address.address,
    port: address.port,
    close: () =>
      new Promise<void>((done, fail) => {
        server.close((error) => (error ? fail(error) : done()));
        server.closeIdleConnections();
      }),
  };
}
