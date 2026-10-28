import { Head, server, signal } from "reze-js";
import * as s from "reze-js/schema";

const state = globalThis as { __lastAction?: string };

export const save = server(s.object({ title: s.string() }), async (input) => {
  const result = `saved:${input.title}`;
  state.__lastAction = result;
  return result;
});

export default function Page() {
  let result = signal("none");
  return (
    <main>
      <Head>
        <title>Actions</title>
      </Head>
      <h1 id="actions">actions</h1>
      <p id="action-id">{save.id}</p>
      <p id="last">{state.__lastAction ?? "none"}</p>
      <form id="form" method="post" action={save.endpoint}>
        <input id="title" name="title" />
        <input type="hidden" name="_return" value="/" />
        <button id="submit" type="submit">
          save
        </button>
      </form>
      <button
        id="rpc"
        type="button"
        onClick={async () => {
          result = await save.call({ title: "rpc" });
        }}
      >
        {result}
      </button>
    </main>
  );
}
