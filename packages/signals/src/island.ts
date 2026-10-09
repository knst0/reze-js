import { createContext, useContext } from "./provide";

export interface Seed {
  readonly values: readonly unknown[];
  readonly rejection: { readonly error: unknown } | undefined;
}

export interface IslandState {
  readonly prefix: string;
  readonly seeds: ReadonlyMap<string, Seed>;
  readonly occurrences: Map<string, number>;
  nextId: number;
}

export const IslandContext = /* @__PURE__ */ createContext<IslandState | undefined>(undefined);

export function takeSeed(siteKey: string): Seed | undefined {
  const island = useContext(IslandContext);
  if (island === undefined) return undefined;
  const occurrence = island.occurrences.get(siteKey) ?? 0;
  island.occurrences.set(siteKey, occurrence + 1);
  return island.seeds.get(`${siteKey}:${occurrence}`);
}
