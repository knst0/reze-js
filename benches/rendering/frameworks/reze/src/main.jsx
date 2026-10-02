import { For, render, selector, signal } from "reze-js";

const adjectives = [
  "pretty",
  "large",
  "big",
  "small",
  "tall",
  "short",
  "long",
  "handsome",
  "plain",
  "quaint",
  "clean",
  "elegant",
  "easy",
  "angry",
  "crazy",
  "helpful",
  "mushy",
  "odd",
  "unsightly",
  "adorable",
  "important",
  "inexpensive",
  "cheap",
  "expensive",
  "fancy",
];
const colors = ["red", "yellow", "blue", "green", "pink", "brown", "purple", "brown", "white", "black", "orange"];
const nouns = ["table", "chair", "house", "bbq", "desk", "car", "pony", "cookie", "sandwich", "burger", "pizza", "mouse", "keyboard"];

const random = (max) => Math.round(Math.random() * 1000) % max;

let nextId = 1;

function buildData(count) {
  const data = [];
  for (let i = 0; i < count; i++) {
    const [label, setLabel] = signal(
      `${adjectives[random(adjectives.length)]} ${colors[random(colors.length)]} ${nouns[random(nouns.length)]}`,
    );
    data.push({ id: nextId++, label, setLabel });
  }
  return data;
}

function Button(props) {
  return (
    <div class="col-sm-6 smallpad">
      <button id={props.id} class="btn btn-primary btn-block" type="button" onClick={props.onClick}>
        {props.text}
      </button>
    </div>
  );
}

function App() {
  const [data, setData] = signal([]);
  const [selected, setSelected] = signal(null);
  const isSelected = selector(selected);
  const run = () => setData(buildData(1000));
  const runLots = () => setData(buildData(10000));
  const add = () => setData((d) => [...d, ...buildData(1000)]);
  const update = () => {
    const d = data();
    for (let i = 0; i < d.length; i += 10) d[i].setLabel((l) => l + " !!!");
  };
  const clear = () => setData([]);
  const swapRows = () => {
    const list = data().slice();
    if (list.length > 998) {
      const item = list[1];
      list[1] = list[998];
      list[998] = item;
      setData(list);
    }
  };
  const remove = (id) =>
    setData((d) =>
      d.toSpliced(
        d.findIndex((row) => row.id === id),
        1,
      ),
    );

  return (
    <div class="container">
      <div class="jumbotron">
        <div class="row">
          <div class="col-md-6">
            <h1>Reze</h1>
          </div>
          <div class="col-md-6">
            <div class="row">
              <Button id="run" text="Create 1,000 rows" onClick={run} />
              <Button id="runlots" text="Create 10,000 rows" onClick={runLots} />
              <Button id="add" text="Append 1,000 rows" onClick={add} />
              <Button id="update" text="Update every 10th row" onClick={update} />
              <Button id="clear" text="Clear" onClick={clear} />
              <Button id="swaprows" text="Swap Rows" onClick={swapRows} />
            </div>
          </div>
        </div>
      </div>
      <table class="table table-hover table-striped test-data">
        <tbody>
          <For each={data()} keyed={(row) => row.id}>
            {(row) => (
              <tr class={isSelected(row().id) ? "danger" : ""}>
                <td class="col-md-1">{row().id}</td>
                <td class="col-md-4">
                  <a onClick={() => setSelected(row().id)}>{row().label()}</a>
                </td>
                <td class="col-md-1">
                  <a onClick={() => remove(row().id)}>
                    <span class="glyphicon glyphicon-remove" aria-hidden="true" />
                  </a>
                </td>
                <td class="col-md-6" />
              </tr>
            )}
          </For>
        </tbody>
      </table>
      <span class="preloadicon glyphicon glyphicon-remove" aria-hidden="true" />
    </div>
  );
}

render(() => <App />, document.getElementById("main"));
