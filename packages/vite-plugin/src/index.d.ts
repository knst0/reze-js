import type { Plugin } from "vite";
export interface Options {
    diagnostics?: {
        /** File every diagnostic, `info` included, is appended to as one JSON line. */
        jsonl?: string;
    };
}
export default function reze(options?: Options): Plugin;
