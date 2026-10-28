import { Loading } from "reze-js";

import { Slow } from "../Slow";

export default function Page() {
  return (
    <main>
      <p id="shell">shell</p>
      <Loading fallback={<p id="slow">loading</p>}>
        <Slow />
      </Loading>
    </main>
  );
}
