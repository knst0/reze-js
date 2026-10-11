import { Head } from "reze-js";

export async function Slow() {
  await new Promise((resolve) => setTimeout(resolve, 300));
  return (
    <>
      <Head>
        <meta name="slow" content="settled" />
      </Head>
      <p id="slow">done</p>
    </>
  );
}

Slow.pending = <p>loading slow</p>;
