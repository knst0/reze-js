import { Suspense } from "reze-js";

export function Loader(props) {
  return (
    <Suspense fallback={<p>loading</p>}>
      <p>{props.label}</p>
    </Suspense>
  );
}
