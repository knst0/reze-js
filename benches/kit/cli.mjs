export function selectNames(all, only, kind) {
  const names = only ? only.split(",") : all;
  for (const name of names) if (!all.includes(name)) throw new Error(`unknown ${kind} "${name}", expected one of ${all.join(", ")}`);
  return names;
}
