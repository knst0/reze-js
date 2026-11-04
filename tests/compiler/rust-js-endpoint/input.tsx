import type { RoutePropsFor } from "reze-js";
import { mutation } from "reze-js/server";

export const addNote = mutation(async (input: { title: string }) => input.title);

export default function Home(props: RoutePropsFor<"/">) {
  return (
    <form action={addNote}>
      <h1>{props.data.title}</h1>
      <input name="title" />
      <button>Add</button>
    </form>
  );
}
