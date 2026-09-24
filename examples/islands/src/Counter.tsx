import { signal } from "reze-js";

export function Counter(props: { start: number }) {
  const [count, setCount] = signal(props.start);
  return (
    <section class="counter">
      <output>{count()}</output>
      <div class="buttons">
        <button onClick={() => setCount((n) => n - 1)}>&minus;1</button>
        <button onClick={() => setCount((n) => n + 1)}>+1</button>
      </div>
    </section>
  );
}
