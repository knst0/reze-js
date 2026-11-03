interface Dispatcher {
  n(): number;
  add(block: unknown[]): void;
}

const shift = (base: number) => (id: string) => id.replace(/^[0-9a-z]+/, (scope) => (parseInt(scope, 36) + base).toString(36));
let added = Promise.resolve();

/** The dispatcher, once its module has run. */
const dispatcher = () =>
  new Promise<Dispatcher>((resolve) => {
    const rz = (window as unknown as { rz?: Dispatcher }).rz;
    if (rz) return resolve(rz);
    addEventListener("rz:ready", () => resolve((window as unknown as { rz: Dispatcher }).rz), { once: true });
  });

/** Moves the streamed part `template` into its `<Loading>` boundary and adds its state block. */
function place(template: HTMLTemplateElement): void {
  const id = template.getAttribute("data-rz-s");
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_COMMENT);
  let start: Comment | undefined;
  while (walker.nextNode()) {
    if ((walker.currentNode as Comment).data === `l${id}`) {
      start = walker.currentNode as Comment;
      break;
    }
  }
  template.remove();
  if (!start) return;
  const content = template.content;
  const script = content.querySelector("script[type=rz]");
  script?.remove();
  const sites = [...content.querySelectorAll("[data-rz]")];
  const marks: Comment[] = [];
  const marker = document.createTreeWalker(content, NodeFilter.SHOW_COMMENT);
  while (marker.nextNode()) {
    if (/^[0-9a-z]+\.[0-9a-z]+$/.test((marker.currentNode as Comment).data)) marks.push(marker.currentNode as Comment);
  }
  while (start.nextSibling && !(start.nextSibling.nodeType === 8 && (start.nextSibling as Comment).data === "/l"))
    start.nextSibling.remove();
  start.nextSibling?.remove();
  start.replaceWith(content);
  if (!script) return;
  added = added.then(async () => {
    const rz = await dispatcher();
    const offset = shift(rz.n());
    for (const site of sites) site.setAttribute("data-rz", offset(site.getAttribute("data-rz")!));
    for (const mark of marks) mark.data = offset(mark.data);
    rz.add(JSON.parse(script.textContent!) as unknown[]);
  });
}

/** Places every streamed part whose template has finished parsing. */
export function flush(): void {
  const loading = document.readyState === "loading";
  for (const template of document.querySelectorAll<HTMLTemplateElement>("template[data-rz-s]")) {
    if (!loading || template.nextSibling) place(template);
  }
}

flush();
new MutationObserver(flush).observe(document, { childList: true, subtree: true });
document.addEventListener("DOMContentLoaded", flush);
