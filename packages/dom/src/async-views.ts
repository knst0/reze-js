import { errored } from "./errored";
import type { JSX } from "./jsx";
import { loading } from "./loading";

/**
 * Views of an async component. With a failure view, `loading` is built inside `errored`'s catch scope: its pending
 * count only sees loads built synchronously inside it, and `errored` only catches errors thrown inside it.
 */
export function asyncViews(
  children: () => JSX.Element,
  pending?: () => JSX.Element,
  failure?: (error: unknown, retry: () => void) => JSX.Element,
): () => JSX.Element {
  if (failure === undefined) {
    return pending === undefined ? children : loading(children, pending);
  }
  return errored(() => loading(children, pending), failure);
}
