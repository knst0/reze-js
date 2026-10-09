import { defineRoute, defineRoutes } from "@rezejs/router";

function Slow() {
  return <p>slow</p>;
}

export const routes = defineRoutes([
  defineRoute({
    path: "/slow",
    preload: () => new Promise<never>(() => {}),
    component: Slow,
  }),
]);
