import { createHandler } from "reze-js/server";

interface AppContext {
  cart: number;
}

export default createHandler({
  context: (request): AppContext => ({ cart: Number(/(?:^|; )cart=(\d+)/.exec(request.headers.get("cookie") ?? "")?.[1] ?? 0) }),
});

declare module "reze-js/server" {
  interface Register {
    context: AppContext;
  }
}
