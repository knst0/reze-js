import { loader } from "reze-js/server";

export const load = loader(async ({ params }: { params: { id: number } }) => params.id);

export default function Home() {
  return <main>Home</main>;
}
