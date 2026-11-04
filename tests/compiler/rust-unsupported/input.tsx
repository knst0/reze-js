import type { Page } from "./index.rs";

export default function Home(props: { data: Page }) {
  return (
    <main>
      <h1>{props.data.title}</h1>
      <p>{props.data.missing}</p>
    </main>
  );
}
