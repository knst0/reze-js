export async function openPage(browser, { initScript, cpuThrottle = 1 } = {}) {
  const context = await browser.newContext();
  if (initScript !== undefined) await context.addInitScript({ path: initScript });
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(error));
  const cdp = await context.newCDPSession(page);
  await cdp.send("Performance.enable");
  if (cpuThrottle > 1) await cdp.send("Emulation.setCPUThrottlingRate", { rate: cpuThrottle });
  return { context, page, cdp, errors };
}

export async function heapBytes(cdp) {
  await cdp.send("HeapProfiler.collectGarbage");
  await cdp.send("HeapProfiler.collectGarbage");
  return (await cdp.send("Runtime.getHeapUsage")).usedSize;
}

export async function taskSeconds(cdp) {
  const { metrics } = await cdp.send("Performance.getMetrics");
  return metrics.find((m) => m.name === "TaskDuration").value;
}

export function assertClean(errors, label) {
  if (errors.length > 0) throw new Error(`${label} page errors:\n${errors.map(String).join("\n")}`);
}
