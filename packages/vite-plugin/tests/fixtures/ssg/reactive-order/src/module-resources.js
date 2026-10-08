import { resource } from "@rezejs/signals/internal/resource";

const moduleRuns = [];
export let moduleBranchValue;
export let moduleCatchValue;

function moduleResource(name, prefix = "") {
  return resource(() => {
    const phase =
      typeof document === "undefined" ? "seed" : document.querySelector(`#module-${name}[data-hydrated]`) === null ? "early" : "live";
    if (typeof document !== "undefined") moduleRuns.push(`${name}:${phase}`);
    return Promise.resolve(`${prefix}${phase}`);
  });
}

if (true) {
  const resource = moduleResource("branch");
  moduleBranchValue = () => resource();
}

const moduleStore = {
  get value() {
    return moduleResource("getter");
  },
};
export const moduleGetter = moduleStore.value;

export class ModuleClass {
  static value = moduleResource("class");
}

const moduleLabel = "outer";
try {
  await Promise.reject({});
} catch ({ label = moduleLabel }) {
  function moduleLabel() {
    return "body";
  }
  const resource = moduleResource("catch", `${label}:`);
  moduleCatchValue = () => resource();
}

export function moduleTrace() {
  moduleBranchValue();
  moduleGetter();
  ModuleClass.value();
  moduleCatchValue();
  return moduleRuns.join(",");
}
