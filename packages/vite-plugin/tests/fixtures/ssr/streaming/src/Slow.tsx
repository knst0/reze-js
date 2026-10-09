export async function Slow() {
  await new Promise((resolve) => setTimeout(resolve, 300));
  return <p id="slow">done</p>;
}

Slow.pending = <p>loading slow</p>;
