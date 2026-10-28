import { Head, signal } from "reze-js";

export default function Home() {
  let n = signal(0);
  return (
    <article>
      <Head>
        <title>Home</title>
      </Head>
      <h1 id="home">home</h1>
      <button id="counter" type="button" onClick={() => (n += 1)}>
        {n}
      </button>
      <div id="widget" persist="widget">
        home widget
      </div>
    </article>
  );
}
