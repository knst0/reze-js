import { readHits } from "../../counter-state";

export default function OnePage() {
  return <p id="hits">{readHits()}</p>;
}
