import { defineRoute, defineRoutes } from "@rezejs/router";

function Bad() {
  return <p>bad</p>;
}

export const routes = defineRoutes([
  defineRoute({
    path: "/bad",
    preload: () => ({ nested: { run: () => 1 } }),
    component: Bad,
  }),
]);
