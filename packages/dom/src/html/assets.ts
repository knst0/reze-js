export const AssetMarker = "\uE000reze-asset:";

export function htmlAsset(id: string): string {
  return `${AssetMarker}${id}\uE000`;
}
