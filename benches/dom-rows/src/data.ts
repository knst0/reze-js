import { signal, type Getter, type Setter } from "reze-js";

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
const colours = [
  "red",
  "yellow",
  "blue",
  "green",
  "pink",
  "brown",
  "purple",
  "brown",
  "white",
  "black",
  "orange",
];
const nouns = [
  "table",
  "chair",
  "house",
  "bbq",
  "desk",
  "car",
  "pony",
  "cookie",
  "sandwich",
  "burger",
  "pizza",
  "mouse",
  "keyboard",
];

export interface Row {
  id: number;
  label: Getter<string>;
  setLabel: Setter<string>;
}

let nextId = 1;
let seed = 1;

function random(max: number): number {
  seed = (seed * 16807) % 2147483647;
  return seed % max;
}

function randomLabel(): string {
  return `${adjectives[random(adjectives.length)]} ${colours[random(colours.length)]} ${nouns[random(nouns.length)]}`;
}

export function buildRows(count: number): Row[] {
  const rows: Row[] = [];
  for (let i = 0; i < count; i++) {
    const [label, setLabel] = signal(randomLabel());
    rows.push({ id: nextId++, label, setLabel });
  }
  return rows;
}
