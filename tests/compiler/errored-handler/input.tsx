import { Errored, signal } from "reze-js";

function Counter() {
  let count = signal(0);
  return <button onClick={() => count++}>Count {count}</button>;
}

export default function Home() {
  return (
    <main>
      <Errored fallback={(error) => <p role="alert">{error.message}</p>}>
        <Counter />
      </Errored>
      <Errored fallback={<p>Static failed</p>}>
        <p>No handlers here</p>
      </Errored>
    </main>
  );
}
