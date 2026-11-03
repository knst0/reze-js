import { Loading } from "reze-js";
import { defer, loader } from "reze-js/server";

const slow = <T,>(value: T) => new Promise<T>((resolve) => setTimeout(() => resolve(value), 500));

export const load = loader(async (): Promise<{ version: string; stars: number }> => ({
  version: "0.4.0",
  stars: defer(slow(42)),
}));

export default function About(props: { data: { version: string; stars: number } }) {
  return (
    <main>
      <h1>About</h1>
      <p>reze-js {props.data.version}</p>
      <Loading fallback={<p>Counting stars…</p>}>
        <p>{props.data.stars} stars</p>
      </Loading>
    </main>
  );
}
