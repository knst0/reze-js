import type { RouteProps } from "@rezejs/router";
import { Head } from "reze-js";

export const route = {
  preload: async ({ params, intent }: { params: { id: string }; intent: string }) => {
    if (typeof document !== "undefined") await fetch(`/preload-probe.json?intent=${intent}`);
    return { title: `Post ${params.id}` };
  },
  info: { tag: "blog-info-canary" },
};

export default function PostPage(props: RouteProps<{ id: string }, { title: string }>) {
  return (
    <article id="post" ref={(node) => node.setAttribute("data-hydrated", "")}>
      <Head>
        <title>{props.data.title}</title>
        <meta name="description" content={`post ${props.data.title}`} />
      </Head>
      <h1 id="post-title">{props.data.title}</h1>
      <p id="post-id">{props.params.id}</p>
    </article>
  );
}
