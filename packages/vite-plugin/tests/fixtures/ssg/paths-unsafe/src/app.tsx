import { defineRoute, defineRoutes } from "@rezejs/router";
import type { RouteProps } from "@rezejs/router";

function ById(props: RouteProps<{ id: string }, unknown>) {
  return <p>{props.params.id}</p>;
}

export const routes = defineRoutes([
  defineRoute({
    path: "/blog/:id",
    preload: ({ params }) => ({ id: params.id }),
    component: ById,
  }),
]);
