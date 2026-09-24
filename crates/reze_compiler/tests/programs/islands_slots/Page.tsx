import { Badge, Toggler } from "./Badge";
import { Card } from "./Card";
import { Counter } from "./Counter";

const NOTE = "static";

export function Page(props) {
  return (
    <main>
      <Card title={props.title} footer={<i>{NOTE}</i>}>
        <Counter start={1} label="nested island" />
      </Card>
    </main>
  );
}

export function Showcase() {
  return (
    <main>
      <Badge start={1} icon={<b>x</b>} />
      <Toggler render={(on) => <b>{on}</b>} />
    </main>
  );
}
