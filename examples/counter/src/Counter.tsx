import { computed, Show, signal } from "reze-js";

export function Counter(props: { step: number }) {
  const [count, setCount] = signal(0);
  const doubled = computed(() => count() * 2);
  return (
    <section class="counter">
      <output class={{ negative: count() < 0 }}>{count()}</output>
      <p>doubled: {doubled()}</p>
      <div class="buttons">
        <button onClick={() => setCount((n) => n - props.step)}>&minus;{props.step}</button>
        <button onClick={() => setCount(0)} disabled={count() === 0}>
          reset
        </button>
        <button onClick={() => setCount((n) => n + props.step)}>+{props.step}</button>
      </div>
      <Show when={count() >= 10}>
        <p class="note">That's a lot of clicks.</p>
      </Show>
    </section>
  );
}
