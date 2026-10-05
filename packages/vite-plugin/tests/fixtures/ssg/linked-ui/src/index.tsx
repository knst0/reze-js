import icon from "./icon.svg?no-inline";

export function PackageBadge() {
  return (
    <aside id="package-badge">
      <img id="package-icon" src={icon} alt="linked package" />
      linked package
    </aside>
  );
}
