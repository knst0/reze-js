import type { Params } from "./types";

const Dashes = /-/g;

function encodeMarker(text: string): string {
  return encodeURIComponent(text).replace(Dashes, "%2D");
}

/**
 * Marker key of one route level. Two documents keep a level on navigation only when their keys match:
 * the key carries the level's identity and everything that makes the server output differ, so a kept
 * level is the one the next document would have rendered the same way.
 */
export function layoutKey(level: number, id: string, params: Params, hasData: boolean, isLeaf: boolean, search: string): string {
  return `l:${encodeMarker(JSON.stringify([level, id, params, hasData ? 1 : 0, isLeaf ? 1 : 0, isLeaf ? search : ""]))}`;
}
