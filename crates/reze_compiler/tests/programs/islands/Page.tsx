import { Counter } from "./Counter";

const START = 5;

export function Page(props) {
  return (
    <main>
      <h1>{props.title}</h1>
      <Counter start={1} label="first" />
      <Counter start={START} label={props.title} />
    </main>
  );
}

export function Layout(props) {
  return (
    <div class="layout">
      <Page title={props.title} />
    </div>
  );
}

export function Broken() {
  return (
    <main>
      <Counter start={() => 1} />
    </main>
  );
}

export const WithChildren = () => (
  <main>
    <Counter start={1}>more</Counter>
  </main>
);
export const WithSpread = (props) => (
  <main>
    <Counter {...props} />
  </main>
);
export const WithRef = () => (
  <main>
    <Counter start={1} ref={undefined} />
  </main>
);
export const WithJsxProp = () => (
  <main>
    <Counter start={1} label={<b>x</b>} />
  </main>
);
