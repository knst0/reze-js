import * as k from "./counter";
import { count, bump, doubled } from "./counter";
export const View = () => (
  <button onClick={bump}>
    <p>{count}</p>
    <i>{doubled()}</i>
    <b>{k.count}</b>
  </button>
);
