import type { SignalNode } from "../signal";

export interface SignalFrame {
  slots: (SignalNode | undefined)[];
  cursor: number;
}

let activeFrame: SignalFrame | undefined;

export function createSignalFrame(): SignalFrame {
  return { slots: [], cursor: 0 };
}

export function runWithSignalFrame<T>(frame: SignalFrame, fn: () => T): T {
  const previous = activeFrame;
  frame.cursor = 0;
  activeFrame = frame;
  try {
    const result = fn();
    frame.slots.length = frame.cursor;
    return result;
  } finally {
    activeFrame = previous;
  }
}

export function activeSignalFrame(): SignalFrame | undefined {
  return activeFrame;
}

export function signalNodeForFrame<T>(frame: SignalFrame, initialValue: T, create: () => SignalNode<T>): SignalNode<T> {
  const index = frame.cursor++;
  const existing = frame.slots[index];
  if (existing !== undefined && preservedShapeMatches(existing.pendingValue, initialValue)) {
    return existing as SignalNode<T>;
  }
  const node = create();
  frame.slots[index] = node;
  return node;
}

function preservedShapeMatches(preserved: unknown, initialValue: unknown): boolean {
  return (
    typeof preserved === typeof initialValue &&
    Array.isArray(preserved) === Array.isArray(initialValue) &&
    (preserved === null) === (initialValue === null)
  );
}
