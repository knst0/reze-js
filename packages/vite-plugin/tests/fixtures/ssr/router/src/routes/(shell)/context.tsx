import type { PreloadArgs, RouteProps } from "@rezejs/router";

export const route = {
  preload: ({ context }: PreloadArgs<{}, { requestId: string }>) => context.requestId,
};

export default function ContextPage(props: RouteProps<{}, string>) {
  return <h1 id="request-scope">{props.data}</h1>;
}
