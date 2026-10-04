import { $signal, island, Portal } from "reze-js";

import { Counter } from "./Counter";
import { ClockBody, EagerBody, NeverBody } from "./bodies";

export default function App() {
  let target = $signal<HTMLElement | null>(null);
  let gone = $signal(false);
  return (
    <main>
      <h1>portals</h1>
      <Portal>
        <p id="modal">modal body</p>
      </Portal>
      <section
        id="slot"
        ref={(el: HTMLElement) => {
          target = el;
        }}
      />
      <Portal mount={target}>
        <p id="custom">custom placed</p>
      </Portal>
      <button id="release" type="button" onClick={() => (target = null)}>
        release
      </button>
      {island("eager", () => EagerBody, {}, () => (
        <p>eager waiting</p>
      ))}
      {island("interaction", () => ClockBody, {}, () => (
        <p id="clock-fallback">clock waiting</p>
      ))}
      <Counter island="interaction" step={2} islandFallback={<p id="counter-fallback">counter waiting</p>} />
      {gone ? null : (
        <>
          {island("interaction", () => NeverBody, {}, () => (
            <p id="cancel-fallback">cancel waiting</p>
          ))}
        </>
      )}
      <button id="cancel" type="button" onClick={() => (gone = true)}>
        cancel
      </button>
    </main>
  );
}
