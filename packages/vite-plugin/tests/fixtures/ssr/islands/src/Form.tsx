import { signal } from "reze-js";

export function Form() {
  let probes = signal(0);
  return (
    <form>
      <input id="name" name="name" type="text" />
      <button id="probe" type="button" onClick={() => (probes += 1)}>
        {probes}
      </button>
    </form>
  );
}
