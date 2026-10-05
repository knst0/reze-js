import { $computed, $signal, effect, lazy, Loading } from "reze-js";

import { buildGreeting } from "./data";
import logoUrl from "./logo.svg?url";
import "./styles.css";

const Late = lazy(() => import("./Late"));

async function Quote() {
  const text = await Promise.resolve("settled quote");
  return <blockquote id="quote">{text}</blockquote>;
}

function SpreadChildren() {
  let reads = 0;
  const props = {
    get children() {
      reads += 1;
      return <span>spread child</span>;
    },
  };
  const content = <section {...props} />;
  return <aside>{content}<output id="spread-reads">{reads}</output></aside>;
}

export default function App() {
  let base = $signal(3);
  let doubled = $computed(base * 2);
  let label = $signal("");
  effect(() => {
    label = `v${base}`;
  });
  let name = $signal("ada");
  return (
    <main>
      <h1>basics</h1>
      <p id="settled">
        {base}:{doubled}:{label}
      </p>
      <p id="greeting">{buildGreeting("basics")}</p>
      <button id="inc" type="button" onClick={() => (base += 1)}>
        {base}
      </button>
      <input id="name" value={name} onInput={(event) => (name = (event.target as HTMLInputElement).value)} />
      <p id="hello">hi {name}</p>
      <Loading fallback={<p>loading quote…</p>}>
        <Quote />
      </Loading>
      <Loading fallback={<p>loading late…</p>}>
        <Late />
      </Loading>
      <img id="logo" src={logoUrl} alt="dot" />
      <SpreadChildren />
    </main>
  );
}
