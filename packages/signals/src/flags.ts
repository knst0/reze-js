// Ported from alien-signals (MIT, Copyright (c) 2024-present Johnson Chu); see graph.ts.

// Flag bits. Plain constants so bundlers inline them.
export const FlagNone = 0;
export const FlagMutable = 1;
export const FlagWatching = 2;
export const FlagRecursedCheck = 4;
export const FlagRecursed = 8;
export const FlagDirty = 16;
export const FlagPending = 32;
/**
 * Set on an owner (effect, scope, computed) whose deps include an owned node.
 * Gates the dispose-children-first path so leaf nodes skip the extra deps walk.
 * Never touched by the propagation algorithm.
 */
export const FlagHasChildEffect = 64;
