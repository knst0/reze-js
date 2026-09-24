import { computed, signal } from "reze-js";

export const [count, setCount] = signal(0);
const hidden = 10;
export const twoReaders = computed(() => count() + 1);
export const outsideJsx = computed(() => count() + 2);
export const usesHidden = computed(() => count() + hidden);
export const viaBarrel = computed(() => count() + 3);
export const escapes = computed(() => count() + 4);
export const usesGlobal = computed(() => Math.max(count(), 0));
