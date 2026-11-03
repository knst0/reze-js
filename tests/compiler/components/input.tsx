import { Card } from "../Card";

const Badge = (props: { label: string; "data-tone": string }) => (
  <span class="badge" data-tone={props["data-tone"]}>
    {props.label}
  </span>
);

export default function Home() {
  const user = { name: "Ann <admin>", role: "owner" };
  return (
    <Card title={`Hi ${user.name}`} footer={<em>footer</em>}>
      <Badge label={user.role} data-tone="info" />
      <p>{user.name}</p>
    </Card>
  );
}
