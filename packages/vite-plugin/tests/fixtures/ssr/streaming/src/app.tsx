import { Counter } from "./Counter";
import { Slow } from "./Slow";
import { Static } from "./Static";

export default function App() {
  return (
    <main>
      <Static />
      <Slow />
      <Counter />
    </main>
  );
}
