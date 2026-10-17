import { readHits } from "../../counter-state";

export default function TwoPage() {
  return <p id="hits">{readHits()}</p>;
}
