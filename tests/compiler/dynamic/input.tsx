import { dynamic } from "reze-js";

function Star(props: { size: number }) {
  return <i class="star">{props.size}</i>;
}

function Heart(props: { size: number }) {
  return <i class="heart">{props.size}</i>;
}

const icons = { star: Star, heart: Heart };

function Icon(props: { kind: "star" | "heart" }) {
  const Glyph = dynamic(icons[props.kind]);
  return <Glyph size={2} />;
}

export default function Home() {
  return (
    <p>
      <Icon kind="star" />
      <Icon kind="heart" />
    </p>
  );
}
