import { store } from "reze-js";

export const [todo, setTodo] = store({ title: "Write M3", meta: { done: false, "due-date": "today" } });
export const [settings] = store({ theme: "dark" });
export const [whole, setWhole] = store({ count: 0 });
