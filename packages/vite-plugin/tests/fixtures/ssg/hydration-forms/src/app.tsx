import { effect, signal } from "reze-js";

type Flags = Record<string, unknown>;

function flags(): Flags {
  return globalThis as unknown as Flags;
}

async function Gate() {
  await new Promise((done) => setTimeout(done, 600));
  return <p id="gate">open</p>;
}

export default function App() {
  let name = signal("server-name");
  let choice = signal("a");
  let city = signal("a");
  let agreed = signal(false);
  let count = signal(0);
  effect(() => {
    name = "prep-name";
    choice = "c";
    city = "c";
    agreed = false;
  });
  return (
    <main>
      <h1>forms</h1>
      <Gate />
      <input id="name" value={name} onInput={(event) => (name = (event.target as HTMLInputElement).value)} />
      <p id="name-out">{name}</p>
      <label>
        <input id="radio-a" type="radio" name="pick" value="a" checked={choice === "a"} onChange={() => (choice = "a")} />a
      </label>
      <label>
        <input id="radio-b" type="radio" name="pick" value="b" checked={choice === "b"} onChange={() => (choice = "b")} />b
      </label>
      <label>
        <input id="radio-c" type="radio" name="pick" value="c" checked={choice === "c"} onChange={() => (choice = "c")} />c
      </label>
      <select id="city" value={city} onChange={(event) => (city = (event.target as HTMLSelectElement).value)}>
        <option value="a">a</option>
        <option value="b">b</option>
        <option value="c">c</option>
      </select>
      <input id="agree" type="checkbox" checked={agreed} onChange={(event) => (agreed = (event.target as HTMLInputElement).checked)} />
      <textarea id="bio" value="hello world" rows={3} />
      <details id="more">
        <summary>more</summary>
        <p>hidden</p>
      </details>
      <input
        id="refcheck"
        ref={(el: HTMLInputElement) => {
          flags().__refNode = el;
        }}
      />
      <button id="count" type="button" onClick={() => (count += 1)}>
        {count}
      </button>
      <button
        id="probe"
        type="button"
        onClick={() => {
          flags().__probed = true;
        }}
      >
        probe
      </button>
      <button
        id="confirm"
        type="button"
        onClick={() => {
          name = "confirmed-name";
          choice = "c";
          city = "c";
          agreed = true;
        }}
      >
        confirm
      </button>
    </main>
  );
}
