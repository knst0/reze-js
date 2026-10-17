import { computed, effect, signal } from "reze-js";

export default function EffectPage() {
  let a = signal(2);
  let b = signal(7);
  let doubledA = computed(a * 2);
  let doubledB = computed(b * 2);
  let logged = signal("");
  effect(() => {
    logged = `${a}/${b}`;
  });
  return (
    <article>
      <p id="effect-derived">
        {doubledA}:{doubledB}
      </p>
      <p id="effect-log">{logged}</p>
      <button id="effect-inc" type="button" onClick={() => ((a += 1), (b += 1))}>
        inc
      </button>
    </article>
  );
}
