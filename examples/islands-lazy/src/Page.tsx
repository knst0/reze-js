import { Counter } from "./Counter";
import { LazyCounter } from "./Lazy";

const features = ["Ships static HTML", "Hydrates one island eagerly", "Loads the rest on view"];

export function Page() {
  return (
    <main>
      <header>
        <nav>
          <strong>Reze · Lazy islands</strong>
        </nav>
      </header>
      <section>
        <h1>Static page, lazy islands</h1>
        <p>This page ships as static HTML; the counters below hydrate when seen.</p>
        <ul>
          {features.map((feature) => (
            <li>{feature}</li>
          ))}
        </ul>
        <Counter start={0} />
        <LazyCounter start={10} island:load="visible" />
        <LazyCounter start={20} island:load="idle" />
      </section>
    </main>
  );
}
