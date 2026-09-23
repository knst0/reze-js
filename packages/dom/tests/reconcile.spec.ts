import { expect, test } from "vitest";

import { reconcileArrays } from "../src/dom";

// Deterministic LCG so failures reproduce.
let seed = 1;
const rand = (n: number) => (seed = (seed * 1103515245 + 12345) % 2 ** 31) % n;

function shuffle<T>(list: T[], swaps: number): void {
  for (let s = 0; s < swaps && list.length > 1; s++) {
    const i = rand(list.length);
    const j = rand(list.length);
    [list[i], list[j]] = [list[j]!, list[i]!];
  }
}

test.each([
  ["moves only", 0, 0],
  ["moves and removals", 3, 0],
  ["moves and inserts", 0, 3],
  ["everything", 3, 3],
])("reconcileArrays turns a into b exactly: %s", (_, removals, inserts) => {
  for (let round = 0; round < 500; round++) {
    const parent = document.createElement("div");
    const head = parent.appendChild(document.createElement("i"));
    const a = Array.from({ length: 1 + rand(12) }, () => document.createElement("b"));
    for (const node of a) parent.appendChild(node);
    const tail = parent.appendChild(document.createElement("i"));

    const b = a.filter(() => rand(10) >= removals);
    shuffle(b, rand(4));
    if (rand(2)) b.reverse();
    for (let k = rand(inserts + 1); k--;) {
      b.splice(rand(b.length + 1), 0, document.createElement("s"));
    }
    if (b.length === 0) b.push(document.createElement("s"));

    reconcileArrays(parent, a.slice(), b);
    expect([...parent.childNodes]).toEqual([head, ...b, tail]);
  }
});
