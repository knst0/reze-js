const Controls = "input,textarea,select";
const Focusable = "input,textarea,select,button,a[href],[tabindex]";

interface Preserved {
  readonly index: number;
  readonly tag: string;
  readonly type: string;
  readonly name: string;
  readonly value?: string;
  readonly checked?: boolean;
  readonly selected?: readonly boolean[];
}

export interface FormSnapshot {
  readonly controls: readonly Preserved[];
  readonly focus?: { readonly index: number; readonly tag: string; readonly start: number | null; readonly end: number | null };
}

function elementsOf(nodes: readonly Node[], selector: string): Element[] {
  const found: Element[] = [];
  for (const node of nodes) {
    if (node.nodeType !== 1) continue;
    const element = node as Element;
    if (element.matches(selector)) found.push(element);
    found.push(...element.querySelectorAll(selector));
  }
  return found;
}

function isDirty(control: Element): boolean {
  if (control instanceof HTMLSelectElement) return [...control.options].some((option) => option.selected !== option.defaultSelected);
  if (control instanceof HTMLInputElement) {
    if (control.type === "checkbox" || control.type === "radio") return control.checked !== control.defaultChecked;
    return control.type !== "file" && control.value !== control.defaultValue;
  }
  return (control as HTMLTextAreaElement).value !== (control as HTMLTextAreaElement).defaultValue;
}

export function snapshotForms(nodes: readonly Node[], active: Element | null): FormSnapshot {
  const controls: Preserved[] = [];
  elementsOf(nodes, Controls).forEach((control, index) => {
    if (!isDirty(control)) return;
    const base = {
      index,
      tag: control.localName,
      type: (control as HTMLInputElement).type ?? "",
      name: (control as HTMLInputElement).name ?? "",
    };
    if (control instanceof HTMLSelectElement) controls.push({ ...base, selected: [...control.options].map((option) => option.selected) });
    else if (control instanceof HTMLInputElement && (control.type === "checkbox" || control.type === "radio")) {
      controls.push({ ...base, checked: control.checked });
    } else controls.push({ ...base, value: (control as HTMLInputElement).value });
  });
  const focusables = elementsOf(nodes, Focusable);
  const focusIndex = active === null ? -1 : focusables.indexOf(active);
  if (focusIndex === -1) return { controls };
  const field = active as HTMLInputElement;
  const hasSelection = typeof field.selectionStart === "number";
  return {
    controls,
    focus: {
      index: focusIndex,
      tag: active!.localName,
      start: hasSelection ? field.selectionStart : null,
      end: hasSelection ? field.selectionEnd : null,
    },
  };
}

export function restoreForms(nodes: readonly Node[], snapshot: FormSnapshot): void {
  const controls = elementsOf(nodes, Controls);
  for (const saved of snapshot.controls) {
    const control = controls[saved.index];
    if (control === undefined || control.localName !== saved.tag) continue;
    if ((control as HTMLInputElement).type !== saved.type || (control as HTMLInputElement).name !== saved.name) continue;
    if (saved.selected !== undefined) {
      const options = (control as HTMLSelectElement).options;
      if (options.length === saved.selected.length) saved.selected.forEach((selected, at) => (options[at]!.selected = selected));
    } else if (saved.checked !== undefined) {
      (control as HTMLInputElement).checked = saved.checked;
    } else if (saved.value !== undefined) {
      (control as HTMLInputElement).value = saved.value;
    }
  }
  const focus = snapshot.focus;
  if (focus === undefined) return;
  const target = elementsOf(nodes, Focusable)[focus.index] as HTMLInputElement | undefined;
  if (target === undefined || target.localName !== focus.tag) return;
  target.focus({ preventScroll: true });
  if (focus.start !== null && focus.end !== null && typeof target.setSelectionRange === "function") {
    try {
      target.setSelectionRange(focus.start, focus.end);
    } catch {
      return;
    }
  }
}
