import { signal, useLocation } from "reze-js";

export default function Note(props: { params: { slug: string } }) {
  const location = useLocation();
  let shared = signal("");
  return (
    <article>
      <head>
        <title>Note {props.params.slug}</title>
      </head>
      <h1>Note {props.params.slug}</h1>
      <a href="/notes/second">Second note</a>
      <button onClick={() => (shared = location.pathname + location.hash)}>Share</button>
      <output>{shared}</output>
    </article>
  );
}
