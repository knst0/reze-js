/*
 * Algorithm from udomdiff (ISC, Andrea Giammarchi) as adapted by dom-expressions (MIT, Ryan Carniato).
 */

/**
 * Turns the sibling run `a` inside `parent` into `b` with few DOM moves: trims common ends, swaps crossed ends and
 * moves runs already in order as one block. `a` must be non-empty and is overwritten.
 */
export function reconcileArrays(parent: Node, a: Node[], b: Node[]): void {
  const bLength = b.length;
  let aEnd = a.length;
  let bEnd = bLength;
  let aStart = 0;
  let bStart = 0;
  const after = a[aEnd - 1]!.nextSibling;
  let map: Map<Node, number> | undefined;

  while (aStart < aEnd || bStart < bEnd) {
    if (a[aStart] === b[bStart]) {
      aStart++;
      bStart++;
      continue;
    }
    while (a[aEnd - 1] === b[bEnd - 1]) {
      aEnd--;
      bEnd--;
    }
    if (aEnd === aStart) {
      const node = bEnd < bLength ? (bStart ? b[bStart - 1]!.nextSibling : b[bEnd - bStart]!) : after;
      while (bStart < bEnd) {
        parent.insertBefore(b[bStart++]!, node);
      }
    } else if (bEnd === bStart) {
      while (aStart < aEnd) {
        if (!map?.has(a[aStart]!)) {
          (a[aStart] as ChildNode).remove();
        }
        aStart++;
      }
    } else if (a[aStart] === b[bEnd - 1] && b[bStart] === a[aEnd - 1]) {
      const node = a[--aEnd]!.nextSibling;
      parent.insertBefore(b[bStart++]!, a[aStart++]!.nextSibling);
      parent.insertBefore(b[--bEnd]!, node);
      a[aEnd] = b[bEnd]!;
    } else {
      if (!map) {
        map = new Map();
        for (let i = bStart; i < bEnd; i++) {
          map.set(b[i]!, i);
        }
      }
      const index = map.get(a[aStart]!);
      if (index == null) {
        (a[aStart++] as ChildNode).remove();
      } else if (bStart < index && index < bEnd) {
        let i = aStart;
        let sequence = 1;
        while (++i < aEnd && i < bEnd) {
          const t = map.get(a[i]!);
          if (t == null || t !== index + sequence) {
            break;
          }
          sequence++;
        }
        if (sequence > index - bStart) {
          const node = a[aStart]!;
          while (bStart < index) {
            parent.insertBefore(b[bStart++]!, node);
          }
        } else {
          parent.replaceChild(b[bStart++]!, a[aStart++]!);
        }
      } else {
        aStart++;
      }
    }
  }
}
