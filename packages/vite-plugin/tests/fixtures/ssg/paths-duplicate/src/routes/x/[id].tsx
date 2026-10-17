import type { RouteProps } from "@rezejs/router";

export default function ById(props: RouteProps<{ id: string }, unknown>) {
  return <p>{props.params.id}</p>;
}
