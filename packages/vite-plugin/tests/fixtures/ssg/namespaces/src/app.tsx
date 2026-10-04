import { $signal } from "reze-js";

function Cells(props: { value: string }) {
  return <><td>a1</td><td>{props.value}</td></>;
}

function Options() {
  return <><option value="a">a</option><option value="b">b</option></>;
}

export default function App() {
  let city = $signal("b");
  let note = $signal("first");
  let raw = $signal("<b>trusted</b>");
  return (
    <main>
      <h1>namespaces</h1>
      <table id="grid">
        <tr>
          <Cells value={note} />
        </tr>
      </table>
      <select id="picker" value={city} onChange={(event) => (city = (event.target as HTMLSelectElement).value)}>
        <Options />
      </select>
      <p id="city-out">{city}</p>
      <textarea id="notes" value={note} onInput={(event) => (note = (event.target as HTMLTextAreaElement).value)} />
      <svg id="art" viewBox="0 0 10 10">
        <circle cx="5" cy="5" r="4" />
        <foreignObject x="0" y="0" width="10" height="10">
          <p>inside svg</p>
        </foreignObject>
      </svg>
      <math id="formula">
        <mi>x</mi>
        <mo>+</mo>
        <mn>1</mn>
      </math>
      <p id="adjacent">
        one{" "}
        {note === "" ? null : "two"}
      </p>
      <p id="empty-dynamic">{note === "never" ? "something" : ""}</p>
      <div id="opaque" innerHTML={raw} />
      <button id="raw" type="button" onClick={() => (raw = "<i>swapped</i>")}>
        swap
      </button>
    </main>
  );
}
