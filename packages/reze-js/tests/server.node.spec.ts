import { expect, test } from "vite-plus/test";

import { serve } from "../src/server/node";

test("serve responds 200 with handler body", async () => {
  const server = await serve(async () => new Response("ok"), { port: 0 });
  try {
    const response = await fetch(`http://127.0.0.1:${server.port}/`);
    expect(response.status).toBe(200);
    expect(await response.text()).toBe("ok");
  } finally {
    await server.close();
  }
});

test("serve responds 500 when handler throws", async () => {
  const server = await serve(
    async () => {
      throw new Error("boom");
    },
    { port: 0 },
  );
  try {
    const response = await fetch(`http://127.0.0.1:${server.port}/`);
    expect(response.status).toBe(500);
  } finally {
    await server.close();
  }
});
