import { catchError, effectScope, signal, untrack } from "@rezejs/signals";
import { renderEffect } from "@rezejs/signals/render";

import type { JSX } from "./jsx";

interface Failure {
  error: unknown;
}

/**
 * Runtime of `<Errored>`: shows `children`, and once anything built inside throws, tears it down and shows
 * `fallback(error, reset)`. `reset` builds `children` again. Errors thrown by `fallback` go to the surrounding handler.
 */
export function errored(children: () => JSX.Element, fallback?: (error: unknown, reset: () => void) => JSX.Element): () => JSX.Element {
  const [attempt, setAttempt] = signal(0);
  const [view, setView] = signal<JSX.Element>(undefined);
  let failure: Failure | undefined;
  const retry = (): void => {
    setAttempt((count) => count + 1);
  };
  const reset = (): void => {
    failure = undefined;
    retry();
  };
  renderEffect(() => {
    attempt();
    let next: JSX.Element = undefined;
    if (failure === undefined) {
      let isBuilding = true;
      const dispose = effectScope(() => {
        next = catchError(children, (error) => {
          if (failure === undefined) {
            failure = { error };
            if (!isBuilding) {
              retry();
            }
          }
        });
      });
      isBuilding = false;
      if (failure !== undefined) {
        dispose();
      }
    }
    if (failure !== undefined) {
      const { error } = failure;
      next = fallback === undefined ? undefined : untrack(() => fallback(error, reset));
    }
    setView(() => next);
  });
  return view;
}
