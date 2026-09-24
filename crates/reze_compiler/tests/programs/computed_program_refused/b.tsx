import { escapes, twoReaders } from "./state";

export const B = () => (
  <p>
    {twoReaders()} {escapes()}
  </p>
);
