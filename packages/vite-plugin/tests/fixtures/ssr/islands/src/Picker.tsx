export function Picker(props: { onPick: (value: string) => void }) {
  return (
    <div>
      <button id="pick-a" type="button" onClick={() => props.onPick("a")}>
        a
      </button>
      <button id="pick-b" type="button" onClick={() => props.onPick("b")}>
        b
      </button>
    </div>
  );
}
