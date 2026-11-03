import { signal } from "reze-js";

function Badge(props: { count: number; label: string }) {
  return (
    <p class={props.count > 2 ? "many" : "few"}>
      {props.label}: {props.count}
    </p>
  );
}

function Stepper(props: { value: number; onStep: () => void }) {
  return (
    <div>
      <output>{props.value}</output>
      <button onClick={() => props.onStep()}>Step</button>
    </div>
  );
}

export default function Home() {
  let count = signal(0);
  return (
    <main>
      <Badge count={count} label="Clicks" />
      <Badge count={count * 2} label="Double" />
      <Stepper value={count} onStep={() => count++} />
      <button onClick={() => count++}>Add</button>
    </main>
  );
}
