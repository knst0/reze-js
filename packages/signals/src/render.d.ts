/**
 * Runs `fn(init)` now and again whenever what it read changes, passing the previous result.
 * A binding that read nothing reactive and created nothing is dropped right away, so static
 * expressions cost no graph node.
 */
export declare function renderEffect<T>(fn: (prev: T) => T, init?: T): void;
