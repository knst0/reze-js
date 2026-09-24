import { expect, test } from "vitest";

import reze from "../src/index";

function handlerOf() {
  const plugin = reze() as unknown as {
    transform: { handler: (code: string, id: string) => unknown };
  };
  return plugin.transform.handler;
}

test("files without JSX pass through as null", () => {
  expect(handlerOf()("const a = 1 < 2;", "a.ts")).toBeNull();
});

test("JSX compiles and carries its map", () => {
  const out = handlerOf()("const a = <div>hi</div>;", "a.tsx") as {
    code: string;
    map: string | null;
  };
  expect(out.code).toContain("<div>hi</div>");
  expect(typeof out.map).toBe("string");
});

test("sourcemap: false skips map generation (C27)", () => {
  const plugin = reze({ sourcemap: false }) as unknown as {
    transform: { handler: (code: string, id: string) => unknown };
  };
  const out = plugin.transform.handler("const a = <div>hi</div>;", "a.tsx") as {
    code: string;
    map: string | null;
  };
  expect(out.code).toBe(handlerOf()("const a = <div>hi</div>;", "a.tsx")!.code);
  expect(out.map).toBeNull();
});

test("syntax errors throw with overlay loc and frame (D01)", () => {
  const id = "src/Broken.tsx";
  const code = "const a = 1;\nconst b = <div>;";
  let thrown: unknown;
  try {
    handlerOf()(code, id);
  } catch (e) {
    thrown = e;
  }
  const err = thrown as Error & {
    loc?: { file: string; line: number; column: number };
    frame?: string;
    id?: string;
  };
  expect(err).toBeInstanceOf(Error);
  // Rust test pins the error to line 2; the overlay must point there, 1-based.
  expect(err.loc).toEqual({ file: id, line: 2, column: expect.any(Number) });
  expect(err.id).toBe(id);
  expect(err.frame).toContain("const b = <div>;");
  expect(err.frame).toContain("^");
});


test("compiler warnings surface as Vite warnings with loc (D03/D04/D05)", () => {
  const seen: unknown[] = [];
  const ctx = { warn(w: unknown) { seen.push(w); } };
  const plugin = reze() as unknown as {
    transform: { handler: (this: unknown, code: string, id: string) => unknown };
  };
  const out = plugin.transform.handler.call(ctx, "const a = <div key=\"x\" />;", "a.tsx");
  expect(out).not.toBeNull();
  expect(seen).toHaveLength(1);
  expect(seen[0]).toMatchObject({
    id: "a.tsx",
    loc: { file: "a.tsx", line: 1, column: expect.any(Number) },
  });
  const first = seen[0];
  if (first && typeof first === "object" && "message" in first) {
    expect(String(first.message)).toContain("`key`");
  } else {
    expect.unreachable("warning carries a message");
  }
});
