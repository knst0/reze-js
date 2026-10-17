import type { RouteProps } from "@rezejs/router";

export const route = {
  preload: ({ params }: { params: { id: string } }) => ({ id: params.id }),
};

export default function ById(props: RouteProps<{ id: string }, unknown>) {
  return <p>{props.params.id}</p>;
}
