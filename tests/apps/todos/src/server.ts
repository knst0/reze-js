import { createHandler } from "reze-js/server";

interface AppContext {
  user: string | undefined;
}

export default createHandler({
  context: (request): AppContext => ({ user: /(?:^|; )user=([^;]+)/.exec(request.headers.get("cookie") ?? "")?.[1] }),
});

declare module "reze-js/server" {
  interface Register {
    context: AppContext;
  }
}
