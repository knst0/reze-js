import { Show } from "reze-js";
import { mutation, useSubmission } from "reze-js/server";
import type { Note } from "virtual:reze-types/app_types";

export const addNote = mutation.external<Note, void>({ revalidate: ["/"] });

export default function Home() {
  const submission = useSubmission(addNote);
  return (
    <form action={addNote}>
      <input name="title" value={submission.input?.title ?? ""} />
      <Show when={submission.issues?.title}>{(issue) => <p role="alert">{issue.message}</p>}</Show>
      <button>Add</button>
    </form>
  );
}
