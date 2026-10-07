import type { ErroredProps, JSX, LoadingProps } from "@rezejs/dom";

export { asyncComputed, boundary, isInBoundary, type AsyncComputed, type AsyncContext, type Boundary } from "@rezejs/signals";
export { lazy, type LazyComponent } from "@rezejs/dom";

export declare function Loading(props: LoadingProps): JSX.Element;

export declare function Errored(props: ErroredProps): JSX.Element;
