import { loader, mutation } from "reze-js/server";

export const load = loader(async () => ({ count: 1 }));

export const reset = mutation(async (input: { id: string }) => input.id);

export default function Page() {
  return <main>Page</main>;
}
