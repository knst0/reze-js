import { signal } from "reze-js";

function Hidden() {
  const [open, setOpen] = signal(false);
  return <details open={open()} onToggle={() => setOpen(!open())} />;
}

export const Panel = () => (
  <aside>
    <Hidden />
  </aside>
);
