import { slugify } from "slugify";

export default function Home() {
  const at = new Date(0);
  return (
    <main>
      <time>{new Intl.DateTimeFormat("de").format(at)}</time>
      <p>{slugify("A title")}</p>
    </main>
  );
}
