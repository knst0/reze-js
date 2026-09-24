import { site } from "./data";

export const Banner = ({ title = site.name, tagline = `${title} docs`, size = 2 }) => (
  <h1 class={size}>
    {title}: {tagline}
  </h1>
);

export function Hint({ label = "Hint", text = document.title }) {
  return <p title={label}>{text}</p>;
}
