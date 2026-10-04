import { defineRoute, defineRoutes } from "@rezejs/router";

export const routes = defineRoutes([
  defineRoute({ path: "/loop-a", redirect: { to: "/loop-b" } }),
  defineRoute({ path: "/loop-b", redirect: { to: "/loop-a" } }),
]);
