import { Portal, render } from "reze-js";

function App() {
  return (
    <Portal>
      <p>portaled</p>
    </Portal>
  );
}

render(() => <App />, document.body);
