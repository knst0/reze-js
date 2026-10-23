import type { RouteProps } from "@rezejs/router";

export const route = {
  preload: ({ params }: { params: { id?: string } }) => ({ id: params.id ?? "none" }),
};

export default function OptPage(props: RouteProps<{ id: string }, { id: string }>) {
  return <p id="opt-id">{props.params.id ?? "none"}</p>;
}
