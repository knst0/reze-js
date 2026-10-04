import { moduleExecution } from "../hydration/execution";
import { HydrationError } from "../hydration/protocol";
import { HtmlSession } from "./session";

let assets: Readonly<Record<string, string>> | undefined;

export function installHtmlAssets(registry: Readonly<Record<string, string>>): () => void {
  if (assets !== undefined) throw new HydrationError("HTML asset registry is already installed");
  assets = registry;
  return () => {
    if (assets === registry) assets = undefined;
  };
}

export function htmlAsset(id: string): string {
  if (assets === undefined || !Object.hasOwn(assets, id) || typeof assets[id] !== "string") {
    throw new HydrationError(`missing client asset URL for ${JSON.stringify(id)}`);
  }
  return assets[id];
}

export function markModule(moduleId: string): void {
  const session = moduleExecution(moduleId);
  if (!(session instanceof HtmlSession)) throw new HydrationError(`HTML module ${JSON.stringify(moduleId)} was not registered before import`);
  session.instances.modules.add(moduleId);
}
