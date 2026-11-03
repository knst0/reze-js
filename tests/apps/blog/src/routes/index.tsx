import { For } from "reze-js";
import { loader } from "reze-js/server";
import type { Home } from "virtual:reze-types/blog_types";

export const load = loader.external<Home>();

export default function Index(props: { data: Home }) {
  return (
    <main>
      <h1>{props.data.title}</h1>
      <ul>
        <For each={props.data.posts} fallback={<li>No posts</li>}>
          {(post, index) => (
            <li class={post.status}>
              {index + 1}. <a href={`/posts/${post.id}`}>{post.title}</a> <span>{post.status}</span>
            </li>
          )}
        </For>
      </ul>
      <p>{props.data.posts.length} posts</p>
    </main>
  );
}
