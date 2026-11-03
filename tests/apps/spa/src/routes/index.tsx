import { computed, signal } from "reze-js";

export default function Home() {
  let count = signal(0);
  const doubled = computed(count * 2);
  return (
    <section>
      <h1>Home</h1>
      <output>{count}</output>
      <p>Doubled: {doubled}</p>
      <button onClick={() => count++}>Add</button>
    </section>
  );
}
