/** FNV-1a64 of `text`, lowercase hex; the compiler checks the same hash before specializing. */
export function profileHash(text: string): string {
  let hash = 0xcbf29ce484222325n;
  const bytes = Buffer.from(text, "utf8");
  for (const byte of bytes) {
    hash ^= BigInt(byte);
    hash = (hash * 0x100000001b3n) & 0xffffffffffffffffn;
  }
  return hash.toString(16).padStart(16, "0");
}
