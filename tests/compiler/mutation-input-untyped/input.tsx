import { mutation } from "reze-js/server";

export const rename = mutation(async (input) => input);

export const remove = mutation(async (input: unknown) => input);

export default function Home() {
  return <main>Home</main>;
}
