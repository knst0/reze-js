import { computed, signal } from "reze-js";

export const [count, setCount] = signal(1);
export const step = 2;
export const doubled = computed(() => count() * step);
export const parity = computed(() => (count() % 2 === 0 ? "even" : "odd"));
const total = computed(() => count() + step);
export { total };
