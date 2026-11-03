import { For, Show, signal } from "reze-js";
import { loader } from "reze-js/server";
import type { Post } from "virtual:reze-types/blog_types";

export const load = loader.external<Post>();

function Rating(props: { value: number; views: bigint }) {
  return (
    <p>
      Rated {props.value}, {props.views} views
    </p>
  );
}

export default function PostPage(props: { data: Post }) {
  let likes = signal(props.data.likes);
  return (
    <main>
      <head>
        <title>{props.data.title}</title>
      </head>
      <h1>{props.data.title}</h1>
      <Show when={props.data.subtitle}>{(subtitle) => <p class="subtitle">{subtitle}</p>}</Show>
      <p>{props.data.body}</p>
      <ul aria-label="Tags">
        <For each={props.data.tags} fallback={<li>Untagged</li>}>
          {(tag) => <li>{tag}</li>}
        </For>
      </ul>
      <Rating value={props.data.rating} views={props.data.views} />
      <p>{props.data.status === "Draft" ? "Draft" : "Published"}</p>
      <p>{props.data.tags.length > 0 && "Tagged"}</p>
      <button onClick={() => likes++}>Like</button>
      <output>{likes} likes</output>
    </main>
  );
}
