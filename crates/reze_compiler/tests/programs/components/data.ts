import { signal } from "reze-js";

export const links = [
  { href: "/", label: "Home" },
  { href: "/docs", label: "Docs" },
];
export const year = 2026;
export const [brand] = signal("Reze");
export const counter = { value: 1 };
counter.value++;
