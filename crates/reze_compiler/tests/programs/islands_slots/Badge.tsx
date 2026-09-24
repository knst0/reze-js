import { signal } from "reze-js";

export function Badge(props) {
  const [count, setCount] = signal(props.start);
  return (
    <button onClick={() => setCount(count() + 1)}>
      {props.icon ? <b>{props.icon}</b> : null} {count()}
    </button>
  );
}

export function Toggler(props) {
  const [on, setOn] = signal(false);
  return <button onClick={() => setOn(!on())}>{props.render(on())}</button>;
}
