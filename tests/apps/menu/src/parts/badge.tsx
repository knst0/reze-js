export function Badge(props: { count: number; label: string }) {
  return (
    <p aria-label={props.label} class={props.count > 2 ? "many" : "few"}>
      {props.count}
    </p>
  );
}

export function Stepper({ value, onStep }: { value: number; onStep: () => void }) {
  return (
    <div>
      <output aria-label="Value">{value}</output>
      <button onClick={() => onStep()}>Step</button>
    </div>
  );
}
