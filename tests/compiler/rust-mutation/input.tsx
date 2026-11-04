import { Show } from "reze-js";
import { useSubmission } from "reze-js/server";
import { addNote } from "./index.rs";

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
