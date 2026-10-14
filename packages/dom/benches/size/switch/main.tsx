import { Match, render, signal, Switch } from "reze-js";

function App() {
  let tab = signal("home");
  return (
    <div onClick={() => (tab = tab === "home" ? "about" : "home")}>
      <Switch fallback={<p>missing</p>}>
        <Match when={tab === "home"}>
          <p>home</p>
        </Match>
        <Match when={tab === "about"}>
          <p>about</p>
        </Match>
      </Switch>
    </div>
  );
}

render(() => <App />, document.body);
