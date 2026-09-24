import { signal } from "reze-js";

export const [title, setTitle] = signal("Reze");
export const [count, setCount] = signal(0);
const [theme, setTheme] = signal("dark");
export { theme, setTheme };
