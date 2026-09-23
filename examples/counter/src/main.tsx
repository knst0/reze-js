import { render } from "reze-js";

import { Counter } from "./Counter";

render(() => <Counter step={1} />, document.getElementById("app")!);
