import { count } from "../cart";

export default function Home() {
  count++;
  return (
    <main>
      <p>{count} items</p>
      <button onClick={() => count++}>Add</button>
    </main>
  );
}
