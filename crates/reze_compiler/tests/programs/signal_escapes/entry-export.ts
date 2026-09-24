import { signal } from "reze-js";

export const [name] = signal("imported by the entry, folded");
export const [shown] = signal("exported by the entry");
