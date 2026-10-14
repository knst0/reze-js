import { island, render } from "reze-js";

function App() {
  return <main>{island("idle", () => import("./widget").then((module) => module.Widget), {})}</main>;
}

render(() => <App />, document.body);
