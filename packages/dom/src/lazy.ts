import { createComponent } from "./component";
import type { JSX } from "./jsx";
import { asyncComponent } from "./loading";

type Component<P> = (props: P) => JSX.Element;

export interface LazyComponent<P> extends Component<P> {
  /** Starts loading the module now; resolves with it. A failed load is forgotten, so the next call retries. */
  preload(): PromiseLike<unknown>;
}

/**
 * A component loaded on first use from `load`, usually `() => import("./Panel")`, taking `default` or the `export`
 * named. Until it arrives, an enclosing `<Loading>` shows its fallback; a failure goes to the surrounding `<Errored>`,
 * whose `reset` loads again. The module is requested once and shared by every instance.
 */
export function lazy<P>(load: () => PromiseLike<{ default: Component<P> }>): LazyComponent<P>;
export function lazy<M, K extends keyof M>(
  load: () => PromiseLike<M>,
  options: { export: K },
): LazyComponent<M[K] extends Component<infer P> ? P : never>;
export function lazy(load: () => PromiseLike<Record<string, unknown>>, options?: { export: string }): LazyComponent<unknown> {
  const exportName = options === undefined ? "default" : options.export;
  let pending: PromiseLike<Record<string, unknown>> | undefined;
  let component: Component<unknown> | undefined;
  const preload = (): PromiseLike<unknown> => {
    if (pending !== undefined) {
      return pending;
    }
    const current = (pending = load());
    current.then(
      (module) => {
        component = module[exportName] as Component<unknown>;
      },
      () => {
        if (pending === current) {
          pending = undefined;
        }
      },
    );
    return current;
  };
  const Lazy = (props: unknown): JSX.Element =>
    component !== undefined
      ? createComponent(component, props)
      : asyncComponent(
          () =>
            preload().then((module) => {
              const exported = (module as Record<string, unknown>)[exportName];
              if (process.env.NODE_ENV !== "production" && typeof exported !== "function") {
                throw new Error(`[reze] lazy: the loaded module has no component export \`${exportName}\``);
              }
              return [exported as Component<unknown>] as const;
            }),
          (loaded) => createComponent(loaded()[0], props),
        );
  return Object.assign(Lazy, { preload });
}
