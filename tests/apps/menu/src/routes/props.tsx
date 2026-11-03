import { For, signal } from "reze-js";

import { Badge, Stepper } from "../parts/badge";

function Total(props: { count: number }) {
  return <Badge count={props.count + 10} label="Total" />;
}

export default function Props() {
  let count = signal(0);
  return (
    <main>
      <Badge count={count} label="Clicks" />
      <Badge count={count * 2} label="Double" />
      <Total count={count} />
      <Stepper value={count} onStep={() => count++} />
      <button onClick={() => count++}>Add</button>
      <For each={[7]}>{(n) => <Badge count={n} label="Fixed" />}</For>
    </main>
  );
}
