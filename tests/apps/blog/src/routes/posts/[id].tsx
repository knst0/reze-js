import { Errored, For, Show, signal } from "reze-js";
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

function Share() {
  let copied = signal(false);
  return (
    <p>
      <button onClick={() => (copied = true)}>Share</button> {copied ? "Link copied" : "Not shared"}
    </p>
  );
}

function Hearts(props: { likes: number }) {
  return <p aria-label="Hearts">{props.likes > 2 ? "Loved" : "Liked"}</p>;
}

function Report() {
  let reports = signal(0);
  return (
    <p>
      <button onClick={() => reports++}>Report</button> {reports} reports
      <button
        onClick={() => {
          throw new Error("Broken report");
        }}
      >
        Break
      </button>
      <button onClick={() => Promise.reject(new Error("Late report"))}>Break later</button>
    </p>
  );
}

export default function PostPage(props: { data: Post }) {
  let likes = signal(props.data.likes);
  let notes = signal(props.data.tags);
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
      <Share />
      <Errored fallback={(error) => <p role="alert">Report failed with {error.status}</p>}>
        <Report />
      </Errored>
      <button onClick={() => likes++}>Like</button>
      <output>{likes} likes</output>
      <Hearts likes={likes} />
      <Show when={likes > 2} fallback={<p>Few likes</p>}>
        <p>Popular with {likes} likes</p>
      </Show>
      <ol aria-label="Notes">
        <For each={notes}>
          {(note, index) => (
            <li>
              {index + 1}: {note}
              <button onClick={() => (notes = notes.filter((_, at) => at !== index))}>Drop {note}</button>
            </li>
          )}
        </For>
      </ol>
      <p aria-label="Words">
        <For each={notes}>{(note) => note}</For>
      </p>
      <button onClick={() => (notes = [...notes, `note ${notes.length + 1}`])}>Note</button>
    </main>
  );
}
