import { User } from "./inner/user";
import { echo } from "./outer/outside";

export function Entry() {
  return (
    <main>
      <User />
      {echo}
    </main>
  );
}
