declare module "virtual:rz-app" {
  const routes: import("./runtime/server").Route[];
  export const root: import("./runtime/server").Route["chain"];
  export const assets: import("./runtime/server").Assets;
  export const build: string;
  export const dev: boolean;
  /** The origin of the Rust dev worker, which runs the route `.rs` loaders, guards, and mutations. */
  export const loaders: string | undefined;
  export default routes;
}

declare module "virtual:rz-routes" {
  /** The page routes: their segments and the route nodes of their chains. */
  export const routes: { segments: import("./runtime/match").Segment[]; chain: number[] }[];
  /** By route node: the names of the params its key holds, and whether its loader reads search. */
  export const keys: Record<number, [string[], boolean, boolean]>;
  export const build: string;
}

declare module "virtual:rz-nav" {
  export const navigate: typeof import("./runtime/navigate").navigate;
  export const prefetch: typeof import("./runtime/navigate").prefetch;
  export const submit: typeof import("./runtime/navigate").submit;
  export const call: typeof import("./runtime/navigate").call;
  export const fault: typeof import("./runtime/navigate").fault;
}

declare module "virtual:rz-chunks" {
  /** The URLs of the client chunks, by their index in the build manifest. */
  const urls: string[];
  export default urls;
  /** Starts the idle preload of the chunks in the viewport. */
  export function watch(rz: import("./runtime/idle").Dispatcher): void;
}
