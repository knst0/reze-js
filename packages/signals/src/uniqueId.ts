let next = 0;

/** An id that is a valid CSS identifier and unique on the page. */
export function createUniqueId(): string {
  return "r" + next++;
}
