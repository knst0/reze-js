import type { QueryClient } from "./index";

declare module "@rezejs/router" {
  interface RouterContext {
    queryClient: QueryClient;
  }
}
