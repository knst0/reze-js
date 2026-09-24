import { renderToString } from "reze-js";
import { Layout } from "./Page";
import { Panel } from "./Hidden";

export const html = renderToString(() => <Layout title="Docs" />);
export const panel = renderToString(() => <Panel />);
