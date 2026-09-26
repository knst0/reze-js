export declare const FlagNone = 0;
export declare const FlagMutable = 1;
export declare const FlagWatching = 2;
export declare const FlagRecursedCheck = 4;
export declare const FlagRecursed = 8;
export declare const FlagDirty = 16;
export declare const FlagPending = 32;
/**
 * Set on an owner whose deps include an owned node (effect, scope, cleanup, provider). Gates the
 * dispose-owned-first walk so leaf computations skip it. Propagation never reads it.
 */
export declare const FlagOwnsChildren = 64;
