const up = (node: unknown, selector: string) => (node as Element | null | undefined)?.closest?.(selector) as any;
const noop = () => {};
let bypass: unknown;

/** The same-origin link a navigation event targets, when the client may handle it. */
const link = (event: Event): HTMLAnchorElement | undefined => {
  const anchor = up(event.target, "a[href]");
  if (!anchor || anchor.origin != location.origin || anchor.hasAttribute("download") || (anchor.target && anchor.target != "_self")) return;
  if (anchor.pathname != location.pathname || anchor.search != location.search || !anchor.hash) return anchor;
};

/**
 * Hands clicks on links and submits of server-tier forms to the navigation module at `url`, and
 * loads it on intent.
 */
export function links(url: string): void {
  /** Imports the navigation module through the dispatcher, which client code also reaches through `window.rzNav`. */
  const nav = ((window as unknown as { rzNav: unknown }).rzNav = () =>
    (window as unknown as { rz: { g(url: string): Promise<unknown> } }).rz.g(url) as Promise<typeof import("virtual:rz-nav")>);

  for (const type of ["pointerenter", "pointerdown", "focusin"]) {
    document.addEventListener(
      type,
      (event) => {
        if (type == "pointerenter" && !/mouse|pen/.test((event as PointerEvent).pointerType)) return;
        const anchor = link(event);
        if (anchor && !(navigator as { connection?: { saveData?: boolean } }).connection?.saveData)
          nav().then((m) => m.prefetch(anchor.href), noop);
        else if (up(event.target, "form")?.action.includes("_m=")) nav().catch(noop);
      },
      true,
    );
  }

  addEventListener("click", (event) => {
    const anchor = link(event);
    if (!anchor || event.defaultPrevented || event.button || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    event.preventDefault();
    nav().then(
      (m) => m.navigate(anchor.href),
      () => location.assign(anchor.href),
    );
  });

  addEventListener("submit", (event) => {
    const form = event.target as HTMLFormElement;
    if (event.defaultPrevented || form == bypass || form.method != "post" || !form.action.includes("_m=")) return;
    event.preventDefault();
    if (form.getAttribute("aria-busy")) return;
    const submitter = event.submitter;
    form.setAttribute("aria-busy", "true");
    nav()
      .then((m) => m.submit(form, submitter))
      .catch(() => {
        bypass = form;
        form.requestSubmit(submitter);
      })
      .finally(() => form.removeAttribute("aria-busy"));
  });

  if (document.querySelector("template[data-rz-client]")) nav().then((m) => m.navigate(location.href, true), noop);
}
