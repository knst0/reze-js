import { renderToString } from "reze-js";

import { Page } from "./Page";

export const html = renderToString(() => <Page />);
