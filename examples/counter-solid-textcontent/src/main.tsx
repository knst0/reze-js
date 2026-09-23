import { render } from "solid-js/web";

import { Counter } from "./Counter";

render(() => <Counter step={1} />, document.getElementById("app")!);
