import type { RouteConfigFor, RoutePropsFor } from "@rezejs/router";
import { paths } from "virtual:reze-routes";

interface Post {
  title: string;
}

export const route = {
  preload: ({ params }) => ({ title: `Post number ${params.id}` }),
} satisfies RouteConfigFor<"/blog/:id", Post>;

export default function PostPage(props: RoutePropsFor<"/blog/:id">) {
  return (
    <article>
      <h2 id="post-title">{props.data.title}</h2>
      <p>id: {props.params.id}</p>
      <a href={paths.blog.byId(Number(props.params.id) + 1)}>Next post</a>
    </article>
  );
}
