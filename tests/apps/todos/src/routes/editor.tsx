import { signal } from "reze-js";
import { loader } from "reze-js/server";

import { render as renders } from "../db";

export const render = "client";

export const load = loader(async (): Promise<{ render: number }> => ({ render: renders() }));

export default function Editor(props: { data: { render: number } }) {
  let words = signal(0);
  return (
    <main>
      <h1>Editor</h1>
      <p>Render {props.data.render}</p>
      <button onClick={() => words++}>Words {words}</button>
    </main>
  );
}
