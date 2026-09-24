import { hydrate } from "reze-js";

import { Page } from "./Page";

hydrate(() => <Page title="Docs" />, document.getElementById("app"));
