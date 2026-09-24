import { hydrate } from "reze-js";

import { Page } from "./Page";

hydrate(() => <Page />, document.getElementById("app")!);
