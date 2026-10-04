export const route = {
  meta: { title: "Lazy" },
  info: { tag: "lazy-info-canary" },
};

export default function LazyPage() {
  return (
    <article>
      <h2 id="lazy-title">lazy loaded</h2>
    </article>
  );
}
