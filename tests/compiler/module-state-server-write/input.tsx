import { cart, count } from "../cart";

export default function Home() {
  count++;
  cart.total = 1;
  cart.items.push("pear");
  const names = cart.items.map((item) => item.toUpperCase());
  return (
    <main>
      <p>
        {count} items, {names.length} names
      </p>
      <button onClick={() => count++}>Add</button>
      <button onClick={() => cart.items.push("apple")}>Apple</button>
    </main>
  );
}
