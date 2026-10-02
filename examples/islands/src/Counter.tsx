import { $signal } from "reze-js";

export function Counter(props: { step: number }) {
  let count = $signal(0);
  return (
    <section class="counter">
      <output>{count}</output>
      <div class="buttons">
        <button onClick={() => (count -= props.step)}>&minus;{props.step}</button>
        <button onClick={() => (count = 0)} disabled={count === 0}>
          reset
        </button>
        <button onClick={() => (count += props.step)}>+{props.step}</button>
      </div>
    </section>
  );
}
