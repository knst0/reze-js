import { signal, store } from "reze-js";

export let count = signal(0);
export const cart = store({ items: [] as string[], total: 0 });
