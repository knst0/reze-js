import routes, { assets, build, root } from "virtual:rz-app";

import { mount } from "./navigate";
import { createApp } from "./server";

/** Mounts a client-only app (`target: "spa"`): renders the current URL in the browser and answers every navigation. */
export const render = (): Promise<void> => mount(createApp(routes, root, { assets, build }));
