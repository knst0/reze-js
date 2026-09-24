import { computed } from "reze-js";

import "./cycle-reader";

export const base = 5;
export const looped = computed(() => base * 2);
