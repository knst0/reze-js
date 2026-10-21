import { Loading } from "reze-js";

import { Counter } from "../Counter";
import { Slow } from "../Slow";
import { Static } from "../Static";

export default function App() {
  return (
    <main>
      <Static />
      <Loading fallback={<p>loading slow</p>}>
        <Slow />
      </Loading>
      <Counter />
    </main>
  );
}
