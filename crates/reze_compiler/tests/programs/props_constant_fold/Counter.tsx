import { signal } from "reze-js";

export function Counter(props) {
  const [count, setCount] = signal(0);
  return (
    <section title={props.title}>
      <button onClick={() => setCount((n) => n - props.step)}>&minus;{props.step}</button>
      <output>{count()}</output>
      <button onClick={() => setCount((n) => n + props.step)}>+{props.step}</button>
      {props.zero && <b>never</b>}
      <Hint step={props.step} />
      <Label text="hi" />
      <Label text="hi" />
    </section>
  );
}

function Hint({ step, label = "step" }) {
  return (
    <small>
      {label}: {step}
    </small>
  );
}

function Label({ text }) {
  return <i title={text}>{text}!</i>;
}

export function Varied(props) {
  return (
    <p>
      {props.size}
      {props.name}
    </p>
  );
}

export function Spread(props) {
  return <p>{props.size}</p>;
}

export function Escaped(props) {
  return <p>{props.size}</p>;
}
