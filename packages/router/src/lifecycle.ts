// Ported from @solidjs/router (MIT, Copyright (c) 2020-2022 Ryan Carniato).
import { onCleanup } from "@rezejs/signals";

import { useLocation, useNavigate, useRouter } from "./routing";
import type { BeforeLeaveEventArgs, BeforeLeaveLifecycle, BeforeLeaveListener } from "./types";

function createBeforeLeave(): BeforeLeaveLifecycle {
  const listeners = new Set<BeforeLeaveListener>();
  const subscribe = (listener: BeforeLeaveListener): (() => void) => {
    listeners.add(listener);
    return () => void listeners.delete(listener);
  };
  let isIgnoring = false;
  const confirm = (to: string | number, options?: BeforeLeaveEventArgs["options"]): boolean => {
    if (isIgnoring) {
      isIgnoring = false;
      return true;
    }
    const event = {
      to,
      options,
      defaultPrevented: false,
      preventDefault: () => void (event.defaultPrevented = true),
    };
    for (const { listener, location, navigate } of listeners) {
      listener({
        to,
        options,
        get defaultPrevented() {
          return event.defaultPrevented;
        },
        preventDefault: () => event.preventDefault(),
        from: location,
        retry: (force?: boolean) => {
          if (force) isIgnoring = true;
          navigate(to as string, { ...options, resolve: false });
        },
      });
    }
    return !event.defaultPrevented;
  };
  return { subscribe, confirm };
}

/**
 * Runs `listener` before leaving the route: `preventDefault()` blocks the navigation,
 * `retry(true)` retries it without running leave handlers again.
 */
export function useBeforeLeave(listener: (event: BeforeLeaveEventArgs) => void): void {
  const router = useRouter();
  const slot = router.beforeLeave;
  const unsubscribe = (slot.current ??= createBeforeLeave()).subscribe({
    listener,
    location: useLocation(),
    navigate: useNavigate(),
  });
  onCleanup(unsubscribe);
}
