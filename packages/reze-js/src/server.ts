import routes, { assets } from "virtual:rz-app";

import { createApp, type Handler, type HandlerOptions } from "./runtime/server";

export type { Handler, HandlerOptions };

export function createHandler(options: HandlerOptions = {}): Handler {
  return createApp(routes, { ...options, assets });
}
