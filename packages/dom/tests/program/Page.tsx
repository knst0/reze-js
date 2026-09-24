import { Counter } from "./Counter";

export function Page() {
  return (
    <main>
      <h1>Program fixture</h1>
      <Counter start={1} label="island" />
      <p>after</p>
    </main>
  );
}
