import { signal } from "reze-js";

export function Counter(props) {
  const [count, setCount] = signal(props.start);
  return (
    <button onClick={() => setCount(count() + 1)}>
      {props.label}: {count()}
    </button>
  );
}
