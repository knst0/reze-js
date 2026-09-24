import { signal } from "reze-js";

export function Card(props) {
  const [open, setOpen] = signal(false);
  return (
    <section>
      <h2>{props.title}</h2>
      <button onClick={() => setOpen(!open())}>{props.children}</button>
      <footer>{props.footer}</footer>
    </section>
  );
}
