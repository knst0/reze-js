import { Head, signal } from "reze-js";

export const route = {
  preload: () => ({ greeting: "home-data" }),
};

export default function Home() {
  let n = signal(0);
  return (
    <article>
      <Head>
        <title>Home</title>
        <meta name="description" content="home page" />
      </Head>
      <h1 id="home-title">home</h1>
      <p id="home-data">home-data</p>
      <button id="home-inc" type="button" onClick={() => (n += 1)}>
        {n}
      </button>
      <a id="to-about" href="/about">
        about
      </a>
      <a id="to-blog-a" href="/blog/a">
        post a
      </a>
      <a id="to-lazy" href="/lazy">
        lazy
      </a>
    </article>
  );
}
