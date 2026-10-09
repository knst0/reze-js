import { currentExecution } from "./instances";
import { HtmlSession } from "./session";
import { RenderError } from "./site";

export function moduleState(moduleId: string, init: () => object): () => object {
  return () => {
    const session = currentExecution();
    if (!(session instanceof HtmlSession)) {
      throw new RenderError(`module state of ${moduleId} was read outside a server render`);
    }
    let state = session.moduleStates.get(moduleId);
    if (state === undefined) {
      state = session.runModule(init);
      session.moduleStates.set(moduleId, state);
    }
    return state;
  };
}

export function moduleProxy(target: () => object): object {
  const resolve = target as () => Record<PropertyKey, unknown>;
  return new Proxy(
    {},
    {
      get: (_, key) => Reflect.get(resolve(), key),
      set: (_, key, value) => Reflect.set(resolve(), key, value),
      has: (_, key) => Reflect.has(resolve(), key),
      ownKeys: () => Reflect.ownKeys(resolve()),
      getOwnPropertyDescriptor: (_, key) => {
        const descriptor = Reflect.getOwnPropertyDescriptor(resolve(), key);
        return descriptor === undefined ? undefined : { ...descriptor, configurable: true };
      },
      deleteProperty: (_, key) => Reflect.deleteProperty(resolve(), key),
    },
  );
}
