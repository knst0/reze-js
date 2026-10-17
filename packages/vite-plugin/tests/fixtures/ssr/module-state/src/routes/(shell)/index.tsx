import { useLocation } from "@rezejs/router";
import { signal, untrack, useContext } from "reze-js";

import { Tone } from "../../tone";

let hits = signal(0);

async function Hits(props: { delay: number }) {
  const delay = props.delay;
  untrack(() => {
    hits += 1;
  });
  await new Promise((resolve) => setTimeout(resolve, delay));
  return <p id="hits">{hits}</p>;
}

function ToneLabel() {
  return <p id="tone">{useContext(Tone)}</p>;
}

export default function Page() {
  const location = useLocation();
  return (
    <>
      <Hits delay={Number(location().query.d ?? 0)} />
      <Tone value="loud">
        <ToneLabel />
      </Tone>
    </>
  );
}
