import { store } from "reze-js";

export const [state, setState] = store({ todos: [{ done: false }], filter: "all" });
