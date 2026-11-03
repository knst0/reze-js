import { Show } from "reze-js";

export const Conditional = Show;

export default function Page() {
  return (
    <Show when={1}>
      <p>ok</p>
    </Show>
  );
}
