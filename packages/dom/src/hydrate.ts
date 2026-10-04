import type { JSX } from "./jsx";
import { HydrationError } from "./hydration/protocol";
import { mountRange } from "./hydration/range";
import { prepareHydration, stagedSession } from "./hydration/session";

/** Claims compiler-generated SSG nodes without replacing them. Failure before commit preserves the root DOM. */
export async function hydrate(code: () => JSX.Element, element: Element): Promise<() => void> {
  const session = stagedSession(element) ?? prepareHydration(element);
  if (!session.preparing) throw new HydrationError("mount root has already been hydrated");
  try {
    session.run(() => mountRange(session, code));
    await session.settle();
    session.run(() => session.commit());
  } catch (error) {
    try {
      session.dispose();
    } catch (cleanupError) {
      throw new AggregateError([error, cleanupError], "hydration and cleanup failed");
    }
    throw error;
  }
  return () => {
    try {
      session.dispose();
    } finally {
      element.textContent = "";
    }
  };
}
