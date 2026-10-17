import type { PreloadArgs, RouteProps } from "@rezejs/router";

interface ShellContext {
  id: number;
  pathname: string;
}

export const route = {
  preload: ({ params, context }: PreloadArgs<{ id: string }, ShellContext>) => `context:${context.id}:${context.pathname}:${params.id}`,
};

export default function ContextPage(props: RouteProps<{ id: string }, string>) {
  return <p id="context-data">{props.data}</p>;
}
