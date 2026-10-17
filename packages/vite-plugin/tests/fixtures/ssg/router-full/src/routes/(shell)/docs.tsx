import { Head, type JSX } from "reze-js";

export default function DocsLayout(props: { children: JSX.Element }) {
  return (
    <section>
      <Head>
        <title>Docs</title>
        <meta name="robots" content="noindex" />
      </Head>
      <h2 id="docs-title">docs</h2>
      {props.children}
    </section>
  );
}
