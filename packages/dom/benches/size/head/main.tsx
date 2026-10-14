import { Head, render } from "reze-js";

function App() {
  return (
    <Head>
      <meta name="description" content="reze" />
      <link rel="canonical" href="/" />
    </Head>
  );
}

render(() => <App />, document.body);
