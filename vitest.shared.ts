import { join } from "node:path";

const packages = join(import.meta.dirname, "packages");

/** Resolves every workspace package to its sources, so tests never read a stale `dist`. */
export const sourceAliases = [
  {
    find: "reze-js/runtime/client",
    replacement: join(packages, "reze-js", "src", "runtime", "client.ts"),
  },
  {
    find: "reze-js/runtime/dispatcher",
    replacement: join(packages, "reze-js", "src", "runtime", "dispatcher.ts"),
  },
  {
    find: "reze-js/runtime/idle",
    replacement: join(packages, "reze-js", "src", "runtime", "idle.ts"),
  },
  {
    find: "reze-js/runtime/links",
    replacement: join(packages, "reze-js", "src", "runtime", "links.ts"),
  },
  {
    find: "reze-js/runtime/navigate",
    replacement: join(packages, "reze-js", "src", "runtime", "navigate.ts"),
  },
  {
    find: "reze-js/runtime/stream",
    replacement: join(packages, "reze-js", "src", "runtime", "stream.ts"),
  },
  {
    find: "reze-js/runtime/render",
    replacement: join(packages, "reze-js", "src", "runtime", "render.ts"),
  },
  {
    find: "reze-js/runtime/server",
    replacement: join(packages, "reze-js", "src", "runtime", "server.ts"),
  },
  { find: "reze-js/types", replacement: join(packages, "reze-js", "src", "types.ts") },
  { find: "reze-js/server", replacement: join(packages, "reze-js", "src", "server.ts") },
  { find: "reze-js", replacement: join(packages, "reze-js", "src", "index.ts") },
];
