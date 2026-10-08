import { signal } from "reze-js";
export let count = signal(0);
export const bump = () => {
  count++;
};
export const doubled = () => count * 2;
