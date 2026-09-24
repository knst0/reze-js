import { Label } from "./label";
import { parity, total } from "./state";

export function Badge() {
  const count = "shadowed";
  return (
    <span class={parity()} title={count}>
      <Label text={total()} />
    </span>
  );
}
