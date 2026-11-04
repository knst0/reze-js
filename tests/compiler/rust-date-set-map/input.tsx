import type { Page } from "./index.rs";

export default function Home(props: { data: Page }) {
  return (
    <main>
      <time datetime={props.data.published.toISOString()}>{props.data.published.getTime()}</time>
      <p>
        {props.data.editors.size} editors{props.data.editors.has("ann") ? ", with ann" : ""}
      </p>
      <p>{props.data.years.has(2024) ? "This year" : "Earlier"}</p>
      <p>
        {props.data.reactions.get("like") ?? 0} likes{props.data.reactions.has("wow") && ", wow"}
      </p>
    </main>
  );
}
