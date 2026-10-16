import { Head } from "reze-js";

import "./lazy.css";

export const route = {
  info: { tag: "lazy-info-canary" },
};

export default function LazyPage() {
  return (
    <article>
      <Head>
        <title>Lazy</title>
      </Head>
      <h2 id="lazy-title">lazy loaded</h2>
    </article>
  );
}
