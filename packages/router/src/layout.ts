import type { Params } from "./types";

const Dashes = /-/g;

function encodeMarker(text: string): string {
  return encodeURIComponent(text).replace(Dashes, "%2D");
}

function dataSignature(data: unknown): string {
  try {
    return (
      JSON.stringify(data, (_key, value: unknown) => {
        const kind = typeof value;
        if (
          kind === "function" ||
          kind === "symbol" ||
          kind === "undefined" ||
          kind === "bigint" ||
          (kind === "number" && !Number.isFinite(value))
        ) {
          throw new TypeError("the level data cannot be compared");
        }
        return value;
      }) ?? "undefined"
    );
  } catch {
    return `random:${Math.random().toString(36).slice(2)}`;
  }
}

/**
 * Marker key of one route level. Two documents keep a level on navigation only when their keys match:
 * the key carries the level's identity, its data signature, and everything that makes the server output
 * differ, so a kept level is the one the next document would have rendered the same way.
 */
export function layoutKey(
  level: number,
  id: string,
  params: Params,
  hasData: boolean,
  isLeaf: boolean,
  search: string,
  data?: unknown,
): string {
  const signature = hasData ? dataSignature(data) : "";
  return `l:${encodeMarker(JSON.stringify([level, id, params, hasData ? 1 : 0, isLeaf ? 1 : 0, isLeaf ? search : "", signature]))}`;
}
