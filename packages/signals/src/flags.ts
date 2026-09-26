// Ported from alien-signals (MIT, Copyright (c) 2024-present Johnson Chu); see graph.ts.

export const FlagNone = 0;
export const FlagMutable = 1;
export const FlagWatching = 2;
export const FlagRecursedCheck = 4;
export const FlagRecursed = 8;
export const FlagDirty = 16;
export const FlagPending = 32;
/**
 * Set on an owner whose deps include an owned node (effect, scope, cleanup, provider). Gates the
 * dispose-owned-first walk so leaf computations skip it. Propagation never reads it.
 */
export const FlagOwnsChildren = 64;
