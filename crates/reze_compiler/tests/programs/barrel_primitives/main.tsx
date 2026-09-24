import { s, store } from "./lib";
import * as lib from "./lib";
import * as R from "reze-js";

const [title] = s("Reze");
const [count] = lib.s(2);
const [label] = R.signal("x");
const [todo, setTodo] = store({ done: false });

export const App = () => (
  <h1 title={label()} onClick={() => setTodo((d) => { d.done = !d.done; })}>
    {title()} {count()} {todo.done ? "yes" : "no"}
  </h1>
);
