import { cache } from "reze-js";

let loads = 0;
const read = cache(
  async () => {
    loads += 1;
    await new Promise((resolve) => setTimeout(resolve, 20));
    return `settled-${loads}`;
  },
  { ttl: 60_000, tags: ["slow"] },
);

export async function Slow() {
  const value = await read();
  return <p id="slow">{value}</p>;
}
