import { defineRoute, defineRoutes } from "@rezejs/router";
import type { RouteProps } from "@rezejs/router";

function ById(props: RouteProps<{ id: string }, unknown>) {
  return <p>{props.params.id}</p>;
}

function Static() {
  return <p>static</p>;
}

export const routes = defineRoutes([defineRoute({ path: "/x/a", component: Static }), defineRoute({ path: "/x/:id", component: ById })]);
