import { createMemo, createSignal, Show } from "solid-js";

export function Counter(props: { step: number }) {
  const [count, setCount] = createSignal(0);
  const doubled = createMemo(() => count() * 2);
  return (
    <section class="counter">
      <output classList={{ negative: count() < 0 }} textContent={count()} />
      <p textContent={`doubled: ${doubled()}`} />
      <div class="buttons">
        <button onClick={() => setCount((n) => n - props.step)} textContent={`-${props.step}`} />
        <button onClick={() => setCount(0)} disabled={count() === 0} textContent="reset" />
        <button onClick={() => setCount((n) => n + props.step)} textContent={`+${props.step}`} />
      </div>
      <Show when={count() >= 10}>
        <p class="note" textContent="That's a lot of clicks." />
      </Show>
    </section>
  );
}
