export const AssetMarker = "\0reze-asset:";

export function htmlAsset(id: string): string {
  return `${AssetMarker}${id}\0`;
}
