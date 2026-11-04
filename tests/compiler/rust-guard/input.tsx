import type { Page } from "./index.rs";

export default function Home(props: { data: Page }) {
  return <h1>{props.data.title}</h1>;
}
