/** Touches the DOM only when `isOn` differs from `wasOn` (absent counts as off); returns `isOn` as the next `wasOn`. */
export function toggleClass(node: Element, token: string, isOn: boolean, wasOn?: boolean): boolean {
  if (isOn !== !!wasOn) {
    node.classList.toggle(token, isOn);
  }
  return isOn;
}
