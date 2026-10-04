import { defineRoute, defineRoutes } from "@rezejs/router";

export const routes = defineRoutes([
  defineRoute({
    path: "/bad",
    preload: () => ({ nested: { run: () => 1 } }),
    component: () => <p>bad</p>,
  }),
]);
