import type { RouteProps } from "@rezejs/router";

export const route = {
  preload: () => undefined,
};

export default function UndefPage(props: RouteProps<Record<string, string>, undefined>) {
  return <p id="undefdata">{props.data === undefined ? "undef" : "has data"}</p>;
}
