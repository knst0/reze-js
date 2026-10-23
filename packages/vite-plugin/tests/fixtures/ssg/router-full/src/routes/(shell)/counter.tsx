import { computed, signal } from "reze-js";

export default function CounterPage() {
  let n = signal(2);
  let double = computed(n * 2);
  return (
    <article>
      <p id="counter-out">
        {n}:{double}
      </p>
      <button id="counter-inc" type="button" onClick={() => (n += 1)}>
        inc
      </button>
    </article>
  );
}
