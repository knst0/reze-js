import { useParams } from "reze-js";

export default function Todo() {
  const params = useParams();
  return (
    <main>
      <head>
        <title>Todo {params.id}</title>
      </head>
      <h1>Todo {params.id}</h1>
    </main>
  );
}
