import type { RouteProps } from "@rezejs/router";

export default function NotFound(props: RouteProps<{ "404": string }>) {
  return <h1>Not found: /{props.params["404"]}</h1>;
}
