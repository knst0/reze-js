import { signal } from "reze-js";
import { loader } from "reze-js/server";

import { render } from "../db";

interface About {
  render: number;
  since: Date;
  visits: bigint;
  tags: Set<string>;
  owners: Map<string, Date>;
}

export const load = loader(async (): Promise<About> => ({
  render: render(),
  since: new Date(Date.UTC(2024, 0, 1)),
  visits: 9007199254740993n,
  tags: new Set(["small"]),
  owners: new Map([["ada", new Date(Date.UTC(2020, 5, 1))]]),
}));

export default function About(props: { data: About }) {
  let facts = signal("");
  return (
    <main>
      <h1>About</h1>
      <p>Render {props.data.render}</p>
      <button
        onClick={() =>
          (facts = `${props.data.since.getUTCFullYear()} ${props.data.visits + 1n} ${props.data.tags.has("small")} ${props.data.owners.get("ada")!.getUTCFullYear()}`)
        }
      >
        Facts
      </button>
      <output>{facts}</output>
    </main>
  );
}
