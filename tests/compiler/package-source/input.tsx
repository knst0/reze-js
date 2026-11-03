import { Counter, Title } from "ui-kit";

export default function Page() {
  return (
    <main>
      <Title text="Library" />
      <Counter start={2} />
    </main>
  );
}
