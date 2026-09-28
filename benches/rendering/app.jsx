import { flush, For, render, signal } from "reze-js";

const adjectives = ["pretty", "large", "big", "small", "tall", "short", "long", "handsome", "plain", "quaint", "clean"];
const colours = ["red", "yellow", "blue", "green", "pink", "brown", "purple", "white", "black", "orange"];
const nouns = ["table", "chair", "house", "bbq", "desk", "car", "pony", "cookie", "sandwich", "burger", "pizza"];

let nextId = 1;

function buildRows(count) {
  const rows = [];
  for (let i = 0; i < count; i++) {
    const id = nextId++;
    const [label, setLabel] = signal(`${adjectives[id % 11]} ${colours[id % 10]} ${nouns[id % 7]}`);
    rows.push({ id, label, setLabel });
  }
  return rows;
}

const appendMarker = (label) => `${label} !!!`;

/** Renders a keyed table into `container`; every operation returns once the DOM is committed. */
export function mount(container) {
  const [rows, setRows] = signal([]);
  const dispose = render(
    () => (
      <table>
        <tbody>
          <For each={rows()} key={(row) => row.id}>
            {(row) => (
              <tr>
                <td>{row().id}</td>
                <td>
                  <a>{row().label()}</a>
                </td>
              </tr>
            )}
          </For>
        </tbody>
      </table>
    ),
    container,
  );

  return {
    create(count) {
      setRows(buildRows(count));
      flush();
    },
    updateEveryTenth() {
      const current = rows();
      for (let i = 0; i < current.length; i += 10) current[i].setLabel(appendMarker);
      flush();
    },
    swap() {
      const next = rows().slice();
      const second = next[1];
      next[1] = next[next.length - 2];
      next[next.length - 2] = second;
      setRows(next);
      flush();
    },
    clear() {
      setRows([]);
      flush();
    },
    rowCount: () => container.querySelectorAll("tr").length,
    dispose,
  };
}
