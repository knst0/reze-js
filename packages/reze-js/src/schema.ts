/** A parsed input guard: `parse` returns the typed value or throws `SchemaError`. */
export interface Schema<T> {
  readonly "~type"?: T;
  parse(value: unknown): T;
}

export class SchemaError extends Error {
  constructor(detail: string) {
    super(detail);
    this.name = "SchemaError";
  }
}

export type Infer<S> = S extends Schema<infer T> ? T : never;

type Shape = Readonly<Record<string, Schema<unknown>>>;

function fail(what: string, value: unknown): never {
  throw new SchemaError(`${what} expected, got ${value === null ? "null" : Array.isArray(value) ? "array" : typeof value}`);
}

function schema<T>(parse: (value: unknown) => T): Schema<T> {
  return { parse };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function string(): Schema<string> {
  return schema((value) => (typeof value === "string" ? value : fail("a string", value)));
}

export function number(): Schema<number> {
  return schema((value) => (typeof value === "number" && Number.isFinite(value) ? value : fail("a finite number", value)));
}

export function boolean(): Schema<boolean> {
  return schema((value) => (typeof value === "boolean" ? value : fail("a boolean", value)));
}

export function literal<T extends string | number | boolean>(expected: T): Schema<T> {
  return schema((value) => (value === expected ? expected : fail(JSON.stringify(expected), value)));
}

export function enumOf<const T extends readonly string[]>(values: T): Schema<T[number]> {
  return schema((value) => {
    if (typeof value !== "string" || !values.includes(value)) fail(`one of ${values.join(", ")}`, value);
    return value as T[number];
  });
}

export function array<T>(item: Schema<T>): Schema<T[]> {
  return schema((value) => {
    if (!Array.isArray(value)) fail("an array", value);
    return value.map((entry, index) => {
      try {
        return item.parse(entry);
      } catch (error) {
        throw new SchemaError(`[${index}]: ${error instanceof Error ? error.message : String(error)}`);
      }
    });
  });
}

export function optional<T>(inner: Schema<T>): Schema<T | undefined> {
  return schema((value) => (value === undefined || value === null ? undefined : inner.parse(value)));
}

export function object<S extends Shape>(shape: S): Schema<{ [K in keyof S]: Infer<S[K]> }> {
  return schema((value) => {
    if (!isRecord(value)) fail("an object", value);
    const out: Record<string, unknown> = {};
    for (const key of Object.keys(shape)) {
      try {
        out[key] = shape[key]!.parse(value[key]);
      } catch (error) {
        throw new SchemaError(`${key}: ${error instanceof Error ? error.message : String(error)}`);
      }
    }
    return out as { [K in keyof S]: Infer<S[K]> };
  });
}
