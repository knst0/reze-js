import type { RouteProps } from "@rezejs/router";

export default function NoDataPage(props: RouteProps<Record<string, string>, unknown>) {
  return <p id="nodata">{props.data === undefined ? "no data" : "has data"}</p>;
}
