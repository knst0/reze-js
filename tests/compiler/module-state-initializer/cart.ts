import { signal } from "reze-js";

export let count = signal(Number(process.env.START));
