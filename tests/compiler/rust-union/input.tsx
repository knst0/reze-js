import { signal } from "reze-js";
import { loader } from "reze-js/server";
import type { Cover, Page } from "virtual:reze-types/app_types";

export const load = loader.external<Page>();

function Zoom(props: { cover: Cover; published: Date }) {
  let width = signal(0);
  return <button onClick={() => (width = props.cover.kind === "Image" ? props.cover.width * 2 : props.published.getTime())}>{width}</button>;
}

export default function Home(props: { data: Page }) {
  return (
    <main>
      <p>{props.data.cover.kind === "Image" ? "Image" : "Plain"}</p>
      <p>
        {props.data.rank[0]} #{props.data.rank[1]}
      </p>
      <Zoom cover={props.data.cover} published={props.data.published} />
    </main>
  );
}
