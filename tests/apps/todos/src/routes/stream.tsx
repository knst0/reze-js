import { For, Loading, signal } from "reze-js";
import { defer, loader } from "reze-js/server";

const later = <T,>(value: T, ms: number) => new Promise<T>((resolve) => setTimeout(() => resolve(value), ms));

export const load = loader(async ({ response }): Promise<{ title: string; comments: string[]; count: number }> => {
  response.headers.set("Cache-Control", "no-store");
  return { title: "Stream", comments: defer(later(["First!", "Second"], 400)), count: defer(later(7, 100)) };
});

function Likes(props: { start: number }) {
  let likes = signal(props.start);
  return <button onClick={() => likes++}>Likes {likes}</button>;
}

export default function StreamPage(props: { data: { title: string; comments: string[]; count: number } }) {
  return (
    <main>
      <h1>{props.data.title}</h1>
      <Loading fallback={<p>Loading comments…</p>}>
        <ul aria-label="Comments">
          <For each={props.data.comments}>{(comment) => <li>{comment}</li>}</For>
        </ul>
      </Loading>
      <Loading fallback={<p>Counting…</p>}>
        <Likes start={props.data.count} />
      </Loading>
    </main>
  );
}
