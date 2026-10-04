import { $signal } from "reze-js";

export function Counter(props: { step: number }) {
  let count = $signal(0);
  return (
    <section class="counter">
      <output id="counter-out">{count}</output>
      <div class="buttons">
        <button id="counter-plus" type="button" onClick={() => (count += props.step)}>
          +{props.step}
        </button>
      </div>
    </section>
  );
}
