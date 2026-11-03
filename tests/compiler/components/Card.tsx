export function Card(props: { title: string; footer: unknown; children: unknown }) {
  return (
    <section>
      <h2>{props.title}</h2>
      {props.children}
      <footer>{props.footer}</footer>
    </section>
  );
}
