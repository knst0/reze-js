import { defineRoute, defineRoutes } from "@rezejs/router";

function About() {
  return <p>about</p>;
}

export const routes = defineRoutes([
  defineRoute({ path: "/gone", redirect: { to: "/nowhere" } }),
  defineRoute({ path: "/about", component: About }),
]);
