import { type Getter } from "./signal";
/**
 * Returns `isSelected(key)`, which is `equals(key, source())` (default `Object.is`) and
 * subscribes its caller to that key's result only: when `source` changes, only the callers
 * whose result flipped re-run, so selecting one row out of n costs O(1) instead of O(n).
 * With a custom `equals` every live key is re-checked on each change, which is O(live keys).
 * Lives as long as the current owner.
 */
export declare function selector<K>(source: Getter<K>, equals?: (key: K, value: K) => boolean): (key: K) => boolean;
