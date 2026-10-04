/** Target adapter for compiler-claimed `<a>` elements outside the page DOM. */
export interface LinkTargetAdapter {
  setAttribute(node: unknown, name: string, value: string | null): void;
  getAttribute(node: unknown, name: string): string | null;
}

let target: LinkTargetAdapter | undefined;

/** Installs the record adapter for SSG preparation; `undefined` restores page-DOM writes. Never imported by app runtime. */
export function installLinkTarget(adapter: LinkTargetAdapter | undefined): void {
  target = adapter;
}

/** The installed record adapter, if any. */
export function linkTarget(): LinkTargetAdapter | undefined {
  return target;
}
