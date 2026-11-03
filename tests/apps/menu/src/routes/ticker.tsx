import { effect, signal } from "reze-js";

export default function Ticker() {
  let started = signal(false);
  effect(() => {
    started = true;
  });
  return (
    <main>
      <div style="height: 3000px">Spacer</div>
      <p data-started={started}>{started ? "Started" : "Waiting"}</p>
    </main>
  );
}
