import { buildPaths, For, signal } from "reze-js";
import { loader } from "reze-js/server";
import type { Home, Summary } from "virtual:reze-types/blog_types";

export const load = loader.external<Home>();

function Drafts(props: { posts: Summary[] }) {
  let drafts = signal(0);
  return (
    <p>
      <button onClick={() => (drafts = props.posts.filter((post) => post.status === "Draft").length)}>Count drafts</button>{" "}
      <output>{drafts} drafts</output>
    </p>
  );
}

export default function Index(props: { data: Home }) {
  return (
    <main>
      <h1>{props.data.title}</h1>
      <ul>
        <For each={props.data.posts} fallback={<li>No posts</li>}>
          {(post, index) => (
            <li class={post.status}>
              {index + 1}. <a href={buildPaths("/posts/:id", { id: post.id })}>{post.title}</a> <span>{post.status}</span>
            </li>
          )}
        </For>
      </ul>
      <p>{props.data.posts.length} posts</p>
      <Drafts posts={props.data.posts} />
    </main>
  );
}
