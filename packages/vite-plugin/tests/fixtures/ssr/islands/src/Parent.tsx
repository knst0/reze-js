import { signal } from "reze-js";

import { Picker } from "./Picker";

export function Parent() {
  let picked = signal("none");
  return (
    <section>
      <Picker onPick={(value) => (picked = value)} />
      <p id="picked">{picked}</p>
    </section>
  );
}
