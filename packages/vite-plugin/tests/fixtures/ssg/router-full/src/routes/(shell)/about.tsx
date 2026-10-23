import { Head } from "reze-js";

export default function About() {
  return (
    <article>
      <Head>
        <title>About</title>
        <link rel="canonical" href="https://example.test/about" />
        <script src="/about-widget.js" defer />
        <link rel="preconnect" href="https://cdn.example.test" />
        <meta property="og:title" content="About the site" />
        <script type="application/ld+json">{'{"@type":"AboutPage"}'}</script>
        <meta name="author" content="Reze" />
      </Head>
      <h1 id="about-title">about</h1>
    </article>
  );
}
