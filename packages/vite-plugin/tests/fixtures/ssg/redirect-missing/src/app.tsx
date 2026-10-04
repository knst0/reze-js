import { defineRoute, defineRoutes } from "@rezejs/router";

export const routes = defineRoutes([
  defineRoute({ path: "/gone", redirect: { to: "/nowhere" } }),
  defineRoute({ path: "/about", component: () => <p>about</p> }),
]);
