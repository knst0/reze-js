import { defineRoute, defineRoutes } from "@rezejs/router";

export const routes = defineRoutes([
  defineRoute({
    path: "/slow",
    preload: () => new Promise<never>(() => {}),
    component: () => <p>slow</p>,
  }),
]);
