import { signal } from "reze-js";

import { Loader } from "./Loader";

export function Widget() {
  const [open, setOpen] = signal(false);
  return (
    <section onClick={() => setOpen(!open())}>
      <Loader label={open() ? "open" : "closed"} />
    </section>
  );
}
