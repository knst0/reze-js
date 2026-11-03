import { signal } from "reze-js";
import { loader } from "reze-js/server";

interface Event {
  title: string;
  at: Date;
  seats: bigint;
  tags: Set<string>;
  prices: Map<string, number>;
  history: Date[];
  ended?: Date;
}

export const load = loader(async (): Promise<Event> => ({
  title: "Launch",
  at: new Date(0),
  seats: 10n,
  tags: new Set(["a"]),
  prices: new Map([["adult", 5]]),
  history: [new Date(0)],
}));

function Countdown({ at }: { at: Date }) {
  let left = signal(0);
  return <button onClick={() => (left = at.getTime() - Date.now())}>{left}</button>;
}

export default function Page(props: { data: Event }) {
  let seats = signal(props.data.seats);
  return (
    <main>
      <button onClick={() => seats--}>{`${seats}`}</button>
      <button onClick={() => console.log(props.data.tags.has("a"), props.data.prices.get("adult"))}>Log</button>
      <button onClick={() => console.log(props.data.history[0].getTime(), props.data.ended?.getTime())}>History</button>
      <Countdown at={props.data.at} />
    </main>
  );
}
