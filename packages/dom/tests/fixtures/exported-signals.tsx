import { signal } from "@rezejs/signals";

export const [extCount, setExtCount] = signal(5);

export function ExternalCounter() {
  return <p>{extCount()}</p>;
}
