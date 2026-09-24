import { hydrate } from "reze-js";

import { Layout } from "./Page";

hydrate(() => <Layout title="Docs" />, document.getElementById("app"));
