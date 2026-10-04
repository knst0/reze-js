import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { Worker } from "node:worker_threads";

import type { BundleOutput } from "./assets";

export interface MaterializedBundle {
  files: Map<string, string>;
  chunks: { fileName: string; code: string }[];
}

export function materializeBundle(outputs: readonly (BundleOutput & { code?: string | undefined })[], dir: string): MaterializedBundle {
  const files = new Map<string, string>();
  const chunks: { fileName: string; code: string }[] = [];
  for (const output of outputs) {
    if (output.type !== "chunk") continue;
    if (output.code === undefined) throw new Error(`[reze] HTML bundle chunk ${JSON.stringify(output.fileName)} has no source to materialize`);
    const target = join(dir, output.fileName);
    mkdirSync(dirname(target), { recursive: true });
    writeFileSync(target, output.code);
    files.set(output.fileName, target);
    chunks.push({ fileName: output.fileName, code: output.code });
  }
  return { files, chunks };
}

export function createTempDir(root: string): string {
  try {
    mkdirSync(join(root, "node_modules"), { recursive: true });
    return mkdtempSync(join(root, "node_modules", ".reze-ssg-"));
  } catch {
    return mkdtempSync(join(tmpdir(), "reze-ssg-"));
  }
}

export function removeTempDir(dir: string): void {
  rmSync(dir, { recursive: true, force: true });
}
export interface WorkerResult<T> {
  ok: boolean;
  result?: T | undefined;
  error?: { message?: string | undefined; stack?: string | undefined } | string | undefined;
}

export function runWorker<T>(file: string, kind: "discover" | "render", input: unknown, timeoutMs: number): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    let settled = false;
    const worker = new Worker(file, { workerData: { kind, input } });
    const watchdog = setTimeout(() => {
      if (settled) return;
      settled = true;
      void worker.terminate().then(() => {
        reject(new Error(`[reze] SSG worker timed out after ${timeoutMs}ms and was terminated`));
      });
    }, timeoutMs + 5000);
    worker.on("message", (message: WorkerResult<T>) => {
      if (settled) return;
      settled = true;
      clearTimeout(watchdog);
      const { result } = message;
      void worker.terminate().then(() => {
        if (message.ok && result !== undefined) resolve(result);
        else {
          const detail = typeof message.error === "string" ? message.error : (message.error?.message ?? "unknown error");
          reject(new Error(`[reze] SSG worker failed: ${detail}`));
        }
      });
    });
    worker.on("error", (error: Error) => {
      if (settled) return;
      settled = true;
      clearTimeout(watchdog);
      reject(new Error(`[reze] SSG worker failed: ${error.message}`));
    });
    worker.on("exit", (code: number) => {
      if (settled) return;
      settled = true;
      clearTimeout(watchdog);
      reject(new Error(`[reze] SSG worker exited with code ${code}`));
    });
  });
}
