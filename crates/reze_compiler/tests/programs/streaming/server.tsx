import { renderToString } from "reze-js";

import { User } from "./User";

export const html = renderToString(() => <User id={1} />);
