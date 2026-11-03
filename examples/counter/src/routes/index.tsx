import { computed, signal } from "reze-js";

export default function Counter() {
  let count = signal(0);
  const doubled = computed(count * 2);
  return (
    <main>
      <h1>Counter</h1>
      <output>{count}</output>
      <p>Doubled: {doubled}</p>
      <button onClick={() => count--}>−</button>
      <button onClick={() => count++}>+</button>
      <button onClick={() => (count = 0)} disabled={count === 0}>
        Reset
      </button>
    </main>
  );
}
