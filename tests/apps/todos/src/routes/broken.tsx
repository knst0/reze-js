import { loader } from "reze-js/server";

export const load = loader(async (): Promise<string> => {
  throw new Error("The database password is hunter2");
});

export default function Broken() {
  return <main>Unreachable</main>;
}
