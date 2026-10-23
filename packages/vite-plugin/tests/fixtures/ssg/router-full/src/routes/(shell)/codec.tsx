import type { RouteProps } from "@rezejs/router";

interface CodecData {
  markup: string;
  u: undefined;
  nil: null;
  t: boolean;
  f: boolean;
  int: number;
  neg: number;
  nan: number;
  inf: number;
  ninf: number;
  big: bigint;
  date: Date;
  arr: Array<number | string | null>;
  rec: { a: number; nested: { b: number } };
  err: Error;
  aliasA: { n: number };
  aliasB: { n: number };
  cyclic: { name: string; self?: unknown };
  proto: Record<string, unknown>;
}

export const route = {
  preload: (): CodecData => {
    const shared = { n: 1 };
    const cyclic: { name: string; self?: unknown } = { name: "c" };
    cyclic.self = cyclic;
    const proto: Record<string, unknown> = Object.create(null);
    proto["__proto__"] = "kept";
    return {
      markup: "</script><script>globalThis.__payloadExecuted=true</script><!--\u2028\u2029",
      u: undefined,
      nil: null,
      t: true,
      f: false,
      int: 42,
      neg: -0,
      nan: NaN,
      inf: Infinity,
      ninf: -Infinity,
      big: 123456789012345678901234567890n,
      date: new Date("2026-10-04T00:00:00.000Z"),
      arr: [1, "two", null],
      rec: { a: 1, nested: { b: 2 } },
      err: Object.assign(new Error("boom"), { cause: "root" }),
      aliasA: shared,
      aliasB: shared,
      cyclic,
      proto,
    };
  },
};

export default function CodecPage(props: RouteProps<Record<string, string>, CodecData>) {
  const data = props.data;
  const errCause: unknown = data.err instanceof Error && "cause" in data.err ? data.err.cause : undefined;
  const checks: Array<[string, boolean]> = [
    ["undef", data.u === undefined],
    ["null", data.nil === null],
    ["bool", data.t === true && data.f === false],
    ["finite", data.int === 42],
    ["negzero", Object.is(data.neg, -0)],
    ["nan", Number.isNaN(data.nan)],
    ["inf", data.inf === Infinity && data.ninf === -Infinity],
    ["bigint", typeof data.big === "bigint" && data.big === 123456789012345678901234567890n],
    ["date", data.date instanceof Date && data.date.toISOString() === "2026-10-04T00:00:00.000Z"],
    ["array", Array.isArray(data.arr) && data.arr.join("|") === "1|two|"],
    ["record", data.rec.nested.b === 2],
    ["error", data.err instanceof Error && data.err.message === "boom" && errCause === "root"],
    ["alias", data.aliasA === data.aliasB && data.aliasA.n === 1],
    ["cycle", data.cyclic.self === data.cyclic],
    ["proto", Object.getPrototypeOf(data.proto) === null && data.proto["__proto__"] === "kept"],
  ];
  return (
    <article id="codec" ref={(node) => node.setAttribute("data-hydrated", "")}>
      {checks.map(([name, ok]) => (
        <p id={`codec-${name}`}>{ok ? "ok" : "FAIL"}</p>
      ))}
      <p id="codec-markup">{data.markup}</p>
    </article>
  );
}
