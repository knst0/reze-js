import { Counter } from "./Counter";

const features = ["Ships static HTML", "Hydrates one island", "Skips the rest"];

export function Page() {
  return (
    <main>
      <header>
        <nav>
          <strong>Reze · Islands</strong>
        </nav>
      </header>
      <section>
        <h1>Static page, live island</h1>
        <p>This page ships as static HTML; only the counter below hydrates.</p>
        <ul>
          {features.map((feature) => (
            <li>{feature}</li>
          ))}
        </ul>
        <Counter start={0} />
      </section>
      <section>
        <h2>How it works</h2>
        <ol>
          <li>The server renders this whole page to HTML.</li>
          <li>The browser hydrates just the counter island.</li>
          <li>Everything else stays inert markup.</li>
        </ol>
      </section>
    </main>
  );
}
