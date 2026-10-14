/// <reference types="vite/client" />

declare const __RZ_TEST_VERSION__: string;

declare module "virtual:reze-routes" {
  import type { PathsTree, RouteDefinition } from "@rezejs/router";
  export const routes: readonly RouteDefinition[];
  export const paths: PathsTree;
}
