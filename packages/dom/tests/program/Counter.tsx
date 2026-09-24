import { signal } from "@rezejs/signals";

import { prefix } from "./signals";
import { profile, setProfile } from "./store";

export function Counter(props: { start: number; label: string }) {
  const [count, setCount] = signal(props.start);
  const advance = (): void => {
    setCount(count() + 1);
    setProfile((draft) => {
      draft.count += 1;
    });
  };
  return (
    <button onClick={advance}>
      {props.label} {prefix()} {count()} {profile.count} {profile.user.name}
    </button>
  );
}
