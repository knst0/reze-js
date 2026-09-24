import { Counter } from "./Counter";

export function Page(props) {
  return (
    <main>
      <h1>{props.title}</h1>
      <Counter start={1} label="eager" />
      <Counter start={2} label="idle" island:load="idle" />
      <Counter start={3} label="visible" island:load="visible" />
      <Counter start={4} label="click" island:load="interaction" />
      <Counter start={5} label="soon" island:load="soon" />
    </main>
  );
}
