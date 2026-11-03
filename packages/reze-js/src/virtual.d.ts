declare module "virtual:rz-app" {
  const routes: import("./runtime/server").Route[];
  export const assets: import("./runtime/server").Assets;
  export default routes;
}
