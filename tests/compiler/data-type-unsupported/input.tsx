import { signal } from "reze-js";
import { loader } from "reze-js/server";

interface Upload {
  name: string;
  file: File;
}

export const load = loader(async (): Promise<Upload> => ({ name: "a", file: new File([], "a") }));

export default function Page(props: { data: Upload }) {
  let shown = signal(false);
  return <button onClick={() => (shown = !shown && props.data.file.size > 0)}>{props.data.name}</button>;
}
