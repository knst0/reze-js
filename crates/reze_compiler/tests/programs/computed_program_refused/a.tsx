import { viaBarrel } from "./barrel";
import { count, outsideJsx, setCount, twoReaders, usesGlobal, usesHidden } from "./state";

export function A() {
  const snapshot = outsideJsx();
  return (
    <p onClick={() => setCount(count() + 1)} title={snapshot}>
      {twoReaders()} {usesHidden()} {viaBarrel()} {usesGlobal()}
    </p>
  );
}
