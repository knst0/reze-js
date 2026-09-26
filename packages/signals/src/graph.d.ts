export interface ReactiveNode {
    deps?: Link;
    depsTail?: Link;
    subs?: Link;
    subsTail?: Link;
    flags: number;
    /** Required on `FlagMutable` nodes: settles pending state and returns whether the value changed. */
    update?(): boolean;
    /** Required on `FlagWatching` nodes: executed by `flush` after being queued. */
    run?(): void;
    /** Called when the last subscriber unlinks. */
    unwatched?(): void;
    /** Present on owned nodes: they are unlinked from their owner before it re-runs. */
    dispose?(): void;
    /** The owner a root or computed was created under; it does not dispose the node. */
    parent?: ReactiveNode | undefined;
}
export interface Link {
    version: number;
    dep: ReactiveNode;
    sub: ReactiveNode;
    prevSub: Link | undefined;
    nextSub: Link | undefined;
    prevDep: Link | undefined;
    nextDep: Link | undefined;
}
/** Unlinks the owned nodes of `sub`, newest first; each disposes itself on unlink. */
export declare function disposeChildren(sub: ReactiveNode): void;
/** Detaches `node` from its deps, newest first, and from its owner. */
export declare function disposeNode(node: ReactiveNode): void;
export declare function disposeAllDepsInReverse(sub: ReactiveNode): void;
/** Unlinks the deps `sub` did not re-read in its latest run. */
export declare function purgeDeps(sub: ReactiveNode): void;
export declare function link(dep: ReactiveNode, sub: ReactiveNode, version: number): void;
export declare function unlink(link: Link, sub?: ReactiveNode): Link | undefined;
/** Marks every transitive subscriber of `link.dep` pending and queues the watching ones. */
export declare function propagate(link: Link, isInnerWrite: boolean): void;
/**
 * Settles the pending deps of `sub`, starting at `link`, and returns whether one of them changed.
 * Re-entrant: `update` runs user getters that may call back into the graph.
 */
export declare function checkDirty(link: Link, sub: ReactiveNode): boolean;
/** Marks the direct subscribers starting at `link` dirty and queues the watching ones. */
export declare function shallowPropagate(link: Link): void;
