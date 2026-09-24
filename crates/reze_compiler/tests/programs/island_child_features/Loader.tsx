import { Loading } from "reze-js";

export function Loader(props) {
  return (
    <Loading fallback={<p>loading</p>}>
      <p>{props.label}</p>
    </Loading>
  );
}
