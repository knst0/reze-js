import { count, doubled as twice, setCount } from "./state";

export function View() {
  return (
    <button onClick={() => setCount(count() + 1)}>
      {count()} x2 = {twice()}
    </button>
  );
}
