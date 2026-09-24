import { renderToString } from "reze-js";

import { Panel } from "./Hidden";
import { Layout } from "./Page";

export const html = renderToString(() => <Layout title="Docs" />);
export const panel = renderToString(() => <Panel />);
