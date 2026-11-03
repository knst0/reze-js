declare module "virtual:rz-app" {
  const routes: import("./runtime/server").Route[];
  export const root: import("./runtime/server").Route["chain"];
  export const assets: import("./runtime/server").Assets;
  export const dev: boolean;
  export default routes;
}
