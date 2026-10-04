import { Clock } from "./Clock";
import { Counter } from "./Counter";

export default function App() {
  return (
    <main>
      <h1>Islands</h1>
      <p>This heading and text are rendered at build time.</p>
      <Counter island="visible" step={1} islandFallback={<p>loading counter…</p>} />
      <Clock island="idle" />
    </main>
  );
}
