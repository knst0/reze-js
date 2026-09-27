import type { RouteConfig, RouteProps } from "@rezejs/router";

interface Post {
  title: string;
}

export const route = {
  preload: ({ params }) => ({ title: `Post number ${params.id}` }),
} satisfies RouteConfig<Post>;

export default function PostPage(props: RouteProps<{ id: string }, Post>) {
  return (
    <article>
      <h2 id="post-title">{props.data.title}</h2>
      <p>id: {props.params.id}</p>
      <a href={`/blog/${Number(props.params.id) + 1}`}>Next post</a>
    </article>
  );
}
