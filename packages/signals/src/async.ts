import { runWithOwner, type Owner } from "./owner";

/**
 * Subscribes to `promise` on behalf of already-disposed-safe owner wiring.
 *
 * The caller captures `owner` via `getOwner()` synchronously (before any
 * `await`) and owns the generation counter: `isCurrent()` must report whether
 * this promise is still the latest for its scope. Stale resolutions are
 * dropped; fresh ones apply their setter back under `owner`, so signals and
 * effects created in the setter are adopted by it and die with it — no manual
 * disposal, no retained DOM.
 */
export function trackAsync<T>(
  owner: Owner | undefined,
  promise: Promise<T>,
  isCurrent: () => boolean,
  setValue: (value: T) => void,
  setError?: (reason: unknown) => void,
): void {
  promise.then(
    (value) => {
      if (isCurrent()) {
        runWithOwner(owner, () => setValue(value));
      }
    },
    (reason) => {
      if (isCurrent()) {
        runWithOwner(owner, () => setError?.(reason));
      }
    },
  );
}
