import { flushSync, For, render, signal } from "reze-js";

import { buildRows, type Row } from "./data";

const [rows, setRows] = signal<Row[]>([]);
const [selected, setSelected] = signal(0);

function Table() {
  return (
    <For each={rows()}>
      {(row) => (
        <tr class={selected() === row().id ? "danger" : ""}>
          <td>{row().id}</td>
          <td>
            <a onClick={() => setSelected(row().id)}>{row().label()}</a>
          </td>
          <td>
            <a>x</a>
          </td>
        </tr>
      )}
    </For>
  );
}

render(() => <Table />, document.getElementById("app")!);

function swapRows(): void {
  const next = rows().slice();
  const second = next[1]!;
  next[1] = next[998]!;
  next[998] = second;
  setRows(next);
}

function removeRow(): void {
  const next = rows().slice();
  next.splice(4, 1);
  setRows(next);
}

function updateEveryTenth(): void {
  const current = rows();
  for (let i = 0; i < current.length; i += 10) {
    const row = current[i]!;
    row.setLabel(`${row.label()} !!!`);
  }
}

Object.assign(window, {
  ops: {
    create1k: () => setRows(buildRows(1000)),
    create10k: () => setRows(buildRows(10000)),
    update10th: updateEveryTenth,
    swap: swapRows,
    select: () => setSelected(rows()[5]!.id),
    remove: removeRow,
    append: () => setRows([...rows(), ...buildRows(1000)]),
    clear: () => setRows([]),
    flush: flushSync,
  },
});
