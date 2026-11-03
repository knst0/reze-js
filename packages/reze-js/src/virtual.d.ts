declare module "virtual:rz-app" {
  const routes: import("./runtime/server").Route[];
  export const root: import("./runtime/server").Route["chain"];
  export const assets: import("./runtime/server").Assets;
  export const build: string;
  export const dev: boolean;
  export default routes;
}

declare module "virtual:rz-routes" {
  /** The page routes: their segments and the route nodes of their chains. */
  export const routes: { segments: import("./runtime/match").Segment[]; chain: number[] }[];
  /** By route node: the names of the params its key holds, and whether its loader reads search. */
  export const keys: Record<number, [string[], boolean]>;
  export const build: string;
}

declare module "virtual:rz-nav" {
  export const navigate: typeof import("./runtime/navigate").navigate;
  export const prefetch: typeof import("./runtime/navigate").prefetch;
  export const submit: typeof import("./runtime/navigate").submit;
}
