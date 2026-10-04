const ControlWrites = new Set([
  "value", "defaultvalue", "checked", "defaultchecked", "selected", "defaultselected",
  "selectedindex", "type", "name", "form", "min", "max", "step", "multiple", "size", "disabled",
]);

export class DirtyForms {
  private readonly dirty = new Set<Element>();
  private readonly ancestors = new Set<Element>();
  private readonly forms = new Set<HTMLFormElement>();
  private readonly radioGroups = new Map<Node, Map<string, HTMLInputElement[]>>();

  constructor(elements: Iterable<Element>) {
    const trees = new Set<Node>();
    for (const element of elements) trees.add(element.getRootNode());
    for (const tree of trees) {
      const controls = (tree as ParentNode).querySelectorAll<HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement>("input,textarea,select");
      for (const control of controls) {
        if (control.localName === "select") {
          const select = control as HTMLSelectElement;
          if (this.selectIsDirty(select)) {
            this.mark(select);
            for (const option of select.options) this.mark(option);
          }
        } else if (control.localName === "textarea") {
          const textarea = control as HTMLTextAreaElement;
          if (textarea.value !== textarea.defaultValue) this.mark(textarea);
        } else {
          const input = control as HTMLInputElement;
          if (input.type === "radio" && input.name !== "") {
            const owner = input.form ?? tree;
            let names = this.radioGroups.get(owner);
            if (names === undefined) this.radioGroups.set(owner, names = new Map());
            let group = names.get(input.name);
            if (group === undefined) names.set(input.name, group = []);
            group.push(input);
          } else if ((input.type === "checkbox" || input.type === "radio") && input.checked !== input.defaultChecked) {
            this.mark(input);
          }
          if (this.inputValueIsDirty(input)) this.mark(input);
        }
      }
    }
    for (const names of this.radioGroups.values()) {
      for (const group of names.values()) {
        let selected: HTMLInputElement | undefined;
        for (const input of group) if (input.defaultChecked) selected = input;
        if (group.some((input) => this.dirty.has(input) || input.checked !== (input === selected))) {
          for (const input of group) this.mark(input);
        }
      }
    }
  }

  allows(element: Element, name: string, value: unknown): boolean {
    const key = name.toLowerCase();
    if ((key === "innerhtml" || key === "textcontent") && this.ancestors.has(element)) return false;
    if (key === "id" && this.forms.has(element as HTMLFormElement)) return false;
    if (this.dirty.has(element) && ControlWrites.has(key)) return false;
    if (element.localName !== "input" || (key !== "name" && key !== "form" && key !== "type")) return true;
    const input = element as HTMLInputElement;
    const type = key === "type" ? String(value ?? "text").toLowerCase() : input.type;
    if (type !== "radio") return true;
    const nameValue = key === "name" ? String(value ?? "") : input.name;
    if (nameValue === "") return true;
    let form = input.form;
    if (key === "form") {
      if (value == null || value === false) form = input.closest("form");
      else {
        const target = input.ownerDocument.getElementById(String(value));
        form = target?.localName === "form" ? target as HTMLFormElement : null;
      }
    }
    const group = this.radioGroups.get(form ?? input.getRootNode())?.get(nameValue);
    return group === undefined || !group.some((member) => this.dirty.has(member));
  }

  private mark(element: Element): void {
    this.dirty.add(element);
    const form = (element as HTMLInputElement).form;
    if (form != null) this.forms.add(form);
    for (let parent: Element | null = element; parent !== null; parent = parent.parentElement) this.ancestors.add(parent);
  }

  private inputValueIsDirty(input: HTMLInputElement): boolean {
    if (input.type === "file") return input.files !== null && input.files.length !== 0;
    if (input.value === input.defaultValue) return false;
    const baseline = input.cloneNode(false) as HTMLInputElement;
    baseline.value = input.defaultValue;
    return input.value !== baseline.value;
  }

  private selectIsDirty(select: HTMLSelectElement): boolean {
    if (select.multiple) {
      for (const option of select.options) if (option.selected !== option.defaultSelected) return true;
      return false;
    }
    let selected = -1;
    for (let index = 0; index < select.options.length; index += 1) {
      if (select.options[index]!.defaultSelected) selected = index;
    }
    if (selected === -1 && select.size <= 1) {
      for (let index = 0; index < select.options.length; index += 1) {
        const option = select.options[index]!;
        const group = option.parentElement;
        if (!option.disabled && !(group?.localName === "optgroup" && (group as HTMLOptGroupElement).disabled)) {
          selected = index;
          break;
        }
      }
    }
    return select.selectedIndex !== selected;
  }
}
