import { Show } from "reze-js";

export default function Page() {
  const ok = true;
  return (
    <Show when={ok} each={1}>
      <p>ok</p>
    </Show>
  );
}
