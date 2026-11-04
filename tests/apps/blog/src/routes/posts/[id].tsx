import { Errored, For, Show, signal } from "reze-js";
import { useSubmission } from "reze-js/server";

import { addComment, type Cover, type Post } from "./[id].rs";

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

function Facts(props: { published: Date; reactions: Map<string, number>; editors: Set<string> }) {
  let facts = signal("");
  return (
    <p>
      <button
        onClick={() =>
          (facts = `${props.published.toISOString()}, ${props.reactions.get("like")} likes, ${props.editors.has("ann") ? "edited by ann" : "unedited"}`)
        }
      >
        Facts
      </button>{" "}
      <span aria-label="Facts">{facts}</span>
    </p>
  );
}

function Poster(props: { cover: Cover; rank: [string, number] }) {
  let caption = signal("");
  return (
    <p>
      <button
        onClick={() => (caption = `${props.cover.kind === "Image" ? props.cover.alt : "No cover"}, ${props.rank[0]} #${props.rank[1]}`)}
      >
        Caption
      </button>{" "}
      <span aria-label="Caption">{caption}</span>
    </p>
  );
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

function Comments(props: { post: Post }) {
  const submission = useSubmission(addComment);
  return (
    <section aria-label="Comments">
      <ul>
        <For each={props.post.comments} fallback={<li>No comments</li>}>
          {(comment) => (
            <li>
              {comment.author}: {comment.body}
            </li>
          )}
        </For>
      </ul>
      <form action={addComment}>
        <input type="hidden" name="post" value={props.post.id} />
        <label>
          Author <input name="author" value={submission.input?.author ?? ""} />
        </label>
        <label>
          Comment <input name="body" value={submission.input?.body ?? ""} />
        </label>
        <label>
          Stars <input name="stars" value={submission.input?.stars ?? ""} />
        </label>
        <Show when={submission.issues?.stars}>{(issue) => <p role="alert">Stars: {issue.message}</p>}</Show>
        <button type="submit">Comment</button>
      </form>
    </section>
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
      <p class="cover">{props.data.cover.kind === "Image" ? "Has a cover" : "No cover"}</p>
      <p aria-label="Summary">
        <time datetime={props.data.published.toISOString()}>{props.data.published.getTime()}</time>, {props.data.reactions.get("like") ?? 0}{" "}
        likes, {props.data.reactions.size} reactions, {props.data.editors.size} editors
        {props.data.editors.has("bo") ? ", with bo" : ""}
      </p>
      <Poster cover={props.data.cover} rank={props.data.rank} />
      <Facts published={props.data.published} reactions={props.data.reactions} editors={props.data.editors} />
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
      <Comments post={props.data} />
    </main>
  );
}
