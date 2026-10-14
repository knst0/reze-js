import { render, signal } from "reze-js";

function App() {
  let count = signal(0);
  return <button onClick={() => (count += 1)}>{count}</button>;
}

render(() => <App />, document.body);
