import type { RouteProps } from "@rezejs/router";

export default function FilesPage(props: RouteProps<{ rest: string }, unknown>) {
  return <p id="rest">{props.params.rest}</p>;
}
