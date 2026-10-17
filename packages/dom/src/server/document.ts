import { hMount } from "../html/helpers";
import { serializeNodes } from "../html/serialize";
import { HtmlSession } from "../html/session";

export async function renderDocument(build: () => unknown, timeoutMs: number): Promise<string> {
  const session = new HtmlSession({ pathname: "/", rootId: "", buildId: "", timeoutMs });
  try {
    session.run(() => hMount(build));
    for (;;) {
      session.flush();
      if (!session.hasPendingWork()) break;
      await session.waitForWork();
    }
    return serializeNodes([session.mount!], { tokenOf: (range) => session.tokenOf(range) });
  } finally {
    session.dispose();
  }
}
