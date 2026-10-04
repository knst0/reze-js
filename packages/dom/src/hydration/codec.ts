export class CodecError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "CodecError";
  }
}

export type WireValue =
  | { readonly tag: "undefined" }
  | { readonly tag: "null" }
  | { readonly tag: "boolean"; readonly value: boolean }
  | { readonly tag: "string"; readonly value: string }
  | { readonly tag: "number"; readonly value: number }
  | { readonly tag: "nan" }
  | { readonly tag: "infinity"; readonly sign: 1 | -1 }
  | { readonly tag: "negzero" }
  | { readonly tag: "bigint"; readonly value: string }
  | { readonly tag: "ref"; readonly id: number };

export type NativeErrorKind =
  | "Error"
  | "EvalError"
  | "RangeError"
  | "ReferenceError"
  | "SyntaxError"
  | "TypeError"
  | "URIError";

export type WireNode =
  | {
      readonly id: number;
      readonly kind: "array";
      readonly length: number;
      readonly items: readonly (readonly [number, WireValue])[];
      readonly extra: readonly (readonly [string, WireValue])[];
    }
  | {
      readonly id: number;
      readonly kind: "record";
      readonly null: boolean;
      readonly entries: readonly (readonly [string, WireValue])[];
    }
  | { readonly id: number; readonly kind: "date"; readonly time: WireValue }
  | {
      readonly id: number;
      readonly kind: "error";
      readonly native: NativeErrorKind;
      readonly name: string;
      readonly message: string;
      readonly hasCause: boolean;
      readonly cause?: WireValue;
    };

export interface GraphFrame {
  readonly nodes: readonly WireNode[];
}

export interface CapturedFrame {
  readonly frame: number;
  readonly values: readonly WireValue[];
}

interface EncodeState {
  slot: string;
  seen: Set<number>;
  nodes: WireNode[];
}

interface Materialized {
  kind: WireNode["kind"];
  value: unknown;
}

const ERROR_CONSTRUCTORS: { [name: string]: (new () => Error) | undefined } = {
  Error,
  EvalError,
  RangeError,
  ReferenceError,
  SyntaxError,
  TypeError,
  URIError,
};

const ERROR_KIND_BY_PROTO = new Map<object, NativeErrorKind>([
  [Error.prototype, "Error"],
  [EvalError.prototype, "EvalError"],
  [RangeError.prototype, "RangeError"],
  [ReferenceError.prototype, "ReferenceError"],
  [SyntaxError.prototype, "SyntaxError"],
  [TypeError.prototype, "TypeError"],
  [URIError.prototype, "URIError"],
]);

const ERROR_OWN_KEYS: Record<string, true> = { cause: true, message: true, name: true, stack: true };

export class FrameEncoder {
  private readonly identifiers = new WeakMap<object, number>();
  private nextIdentifier = 0;
  private readonly box: GraphFrame[] = [];

  constructor(private readonly pathname: string) {}

  get frames(): readonly GraphFrame[] {
    return this.box;
  }

  capture(slotId: string, values: readonly unknown[]): CapturedFrame {
    if (!Array.isArray(values)) {
      throw new CodecError(
        `reze: cannot capture hydration slot "${slotId}" for route "${this.pathname}": values must be an array`,
      );
    }
    const state: EncodeState = { slot: slotId, seen: new Set<number>(), nodes: [] };
    const roots = values.map((value, index) => this.encodeValue(value, state, [index]));
    for (const root of roots) Object.freeze(root);
    const frame: GraphFrame = { nodes: Object.freeze(state.nodes.map(freezeNode)) };
    Object.freeze(frame);
    this.box.push(frame);
    return { frame: this.box.length - 1, values: Object.freeze(roots) };
  }

  private encodeValue(value: unknown, state: EncodeState, path: readonly (string | number)[]): WireValue {
    if (value === undefined) return { tag: "undefined" };
    if (value === null) return { tag: "null" };
    switch (typeof value) {
      case "undefined":
        return { tag: "undefined" };
      case "boolean":
        return { tag: "boolean", value };
      case "string":
        return { tag: "string", value };
      case "number":
        return encodeNumber(value);
      case "bigint":
        return { tag: "bigint", value: value.toString() };
      case "function":
      case "symbol":
        throw this.unsupported(state.slot, path, describe(value));
      case "object":
        return this.encodeObject(value, state, path);
      default:
        throw this.unsupported(state.slot, path, describe(value));
    }
  }

  private encodeObject(value: object, state: EncodeState, path: readonly (string | number)[]): WireValue {
    let id = this.identifiers.get(value);
    if (id === undefined) {
      id = this.nextIdentifier;
      this.nextIdentifier += 1;
      this.identifiers.set(value, id);
    }
    if (state.seen.has(id)) return { tag: "ref", id };
    state.seen.add(id);
    state.nodes.push(this.snapshotObject(value, id, state, path));
    return { tag: "ref", id };
  }

  private snapshotObject(
    value: object,
    id: number,
    state: EncodeState,
    path: readonly (string | number)[],
  ): WireNode {
    if (Array.isArray(value)) return this.snapshotArray(value, id, state, path);
    if (Object.prototype.toString.call(value) === "[object Module]") {
      throw this.unsupported(state.slot, path, "module namespace");
    }
    const proto: unknown = Object.getPrototypeOf(value);
    if (proto === Date.prototype) return this.snapshotDate(value as Date, id, state, path);
    if (value instanceof Error) return this.snapshotError(value, id, state, path);
    if (proto === Object.prototype || proto === null) {
      return this.snapshotRecord(value as Record<string, unknown>, id, state, path);
    }
    throw this.unsupported(state.slot, path, describe(value));
  }

  private snapshotArray(
    value: unknown[],
    id: number,
    state: EncodeState,
    path: readonly (string | number)[],
  ): WireNode {
    if (Object.getPrototypeOf(value) !== Array.prototype) {
      throw this.unsupported(state.slot, path, describe(value));
    }
    if (Object.getOwnPropertySymbols(value).length > 0) {
      throw this.unsupported(state.slot, path, "array with symbol properties");
    }
    const length = value.length;
    const record = value as unknown as Record<string, unknown>;
    const items: Array<readonly [number, WireValue]> = [];
    const extra: Array<readonly [string, WireValue]> = [];
    for (const key of Object.keys(value)) {
      if (isArrayIndexKey(key)) {
        const index = Number(key);
        if (index >= length) {
          throw this.unsupported(state.slot, [...path, key], `array index ${key} is out of bounds`);
        }
        items.push([index, this.encodeValue(record[key], state, [...path, index])]);
      } else {
        extra.push([key, this.encodeValue(record[key], state, [...path, key])]);
      }
    }
    return { id, kind: "array", length, items, extra };
  }

  private snapshotDate(
    value: Date,
    id: number,
    state: EncodeState,
    path: readonly (string | number)[],
  ): WireNode {
    const extra = Object.getOwnPropertyNames(value)[0];
    if (extra !== undefined) {
      throw this.unsupported(state.slot, [...path, extra], `Date with unsupported own property ${JSON.stringify(extra)}`);
    }
    if (Object.getOwnPropertySymbols(value).length > 0) {
      throw this.unsupported(state.slot, path, "Date with symbol properties");
    }
    return { id, kind: "date", time: encodeNumber(value.getTime()) };
  }

  private snapshotError(
    value: Error,
    id: number,
    state: EncodeState,
    path: readonly (string | number)[],
  ): WireNode {
    const proto: unknown = Object.getPrototypeOf(value);
    const native = typeof proto === "object" && proto !== null ? ERROR_KIND_BY_PROTO.get(proto) : undefined;
    if (native === undefined) {
      throw this.unsupported(state.slot, path, describe(value));
    }
    for (const key of Object.keys(value)) {
      if (!Object.hasOwn(ERROR_OWN_KEYS, key)) {
        throw this.unsupported(state.slot, [...path, key], `Error with unsupported own property ${JSON.stringify(key)}`);
      }
    }
    if (Object.getOwnPropertySymbols(value).length > 0) {
      throw this.unsupported(state.slot, path, "Error with symbol properties");
    }
    const name: unknown = value.name;
    if (typeof name !== "string") {
      throw this.unsupported(state.slot, [...path, "name"], "Error with unsupported name");
    }
    const message: unknown = value.message;
    if (typeof message !== "string") {
      throw this.unsupported(state.slot, [...path, "message"], "Error with unsupported message");
    }
    if (!Object.hasOwn(value, "cause")) {
      return { id, kind: "error", native, name, message, hasCause: false };
    }
    return {
      id,
      kind: "error",
      native,
      name,
      message,
      hasCause: true,
      cause: this.encodeValue(value.cause, state, [...path, "cause"]),
    };
  }

  private snapshotRecord(
    value: Record<string, unknown>,
    id: number,
    state: EncodeState,
    path: readonly (string | number)[],
  ): WireNode {
    if (Object.getOwnPropertySymbols(value).length > 0) {
      throw this.unsupported(state.slot, path, "record with symbol properties");
    }
    const entries: Array<readonly [string, WireValue]> = [];
    for (const key of Object.keys(value)) {
      entries.push([key, this.encodeValue(value[key], state, [...path, key])]);
    }
    return { id, kind: "record", null: Object.getPrototypeOf(value) === null, entries };
  }

  private unsupported(slot: string, path: readonly (string | number)[], detail: string): CodecError {
    return new CodecError(
      `reze: unsupported hydration value ${detail} at ${formatPath(path)} (slot "${slot}", route "${this.pathname}")`,
    );
  }
}

export class FrameDecoder {
  private readonly frames: GraphFrame[];
  private readonly frameIds: Array<Set<number>> = [];
  private readonly known = new Set<number>();
  private readonly shells = new Map<number, Materialized>();

  constructor(frames: readonly unknown[]) {
    if (!Array.isArray(frames)) {
      throw new CodecError("reze: malformed hydration data: frames must be an array");
    }
    const validated = frames.map((frame, index) => validateFrame(frame, index));
    const kinds = new Map<number, WireNode["kind"]>();
    for (let index = 0; index < validated.length; index += 1) {
      const frame = validated[index];
      const ids = new Set<number>();
      for (const node of frame.nodes) {
        this.known.add(node.id);
        ids.add(node.id);
        const prev = kinds.get(node.id);
        if (prev === undefined) {
          kinds.set(node.id, node.kind);
        } else if (prev !== node.kind) {
          throw malformed(`frame ${index}`, `node ${node.id} changes from ${prev} to ${node.kind}`);
        }
      }
      this.frameIds.push(ids);
    }
    this.frames = validated;
  }

  validate(value: unknown, frameIndex: number): asserts value is WireValue {
    if (!Number.isInteger(frameIndex) || frameIndex < 0 || frameIndex >= this.frames.length) {
      throw new CodecError(`reze: unknown hydration frame ${repr(frameIndex)}`);
    }
    validateWireValue(value, `frame ${frameIndex} value`, this.frameIds[frameIndex]);
  }

  apply(frameIndex: number): void {
    if (!Number.isInteger(frameIndex) || frameIndex < 0 || frameIndex >= this.frames.length) {
      throw new CodecError(`reze: unknown hydration frame ${repr(frameIndex)}`);
    }
    const frame = this.frames[frameIndex];
    for (const node of frame.nodes) {
      const shell = this.shells.get(node.id);
      if (shell === undefined) {
        this.shells.set(node.id, { kind: node.kind, value: createShell(node) });
      } else if (shell.kind !== node.kind) {
        throw new CodecError(
          `reze: malformed hydration data at frame ${frameIndex}: node ${node.id} changes from ${shell.kind} to ${node.kind}`,
        );
      }
    }
    for (const node of frame.nodes) {
      const shell = this.shells.get(node.id);
      if (shell === undefined) {
        throw new CodecError(`reze: hydration node ${node.id} in frame ${frameIndex} was not materialized`);
      }
      fillNode(shell.value, node, this.shells, frameIndex);
    }
  }

  read(value: WireValue): unknown {
    validateWireValue(value, "hydration value", null);
    if (value.tag === "ref") {
      const shell = this.shells.get(value.id);
      if (shell !== undefined) return shell.value;
      if (this.known.has(value.id)) {
        throw new CodecError(`reze: hydration reference ${value.id} has not been materialized yet`);
      }
      throw new CodecError(`reze: unknown hydration reference ${value.id}`);
    }
    return resolveWire(value, this.shells, -1);
  }
}

export function encodePayloadJson(value: unknown): string {
  const json = JSON.stringify(value);
  if (typeof json !== "string") {
    throw new CodecError("reze: cannot encode hydration payload as JSON");
  }
  return json.replace(/[<>&\u2028\u2029]/g, escapePayloadChar);
}

function escapePayloadChar(char: string): string {
  if (char === "<") return "\\u003c";
  if (char === ">") return "\\u003e";
  if (char === "&") return "\\u0026";
  const code = char.charCodeAt(0);
  if (code === 8232) return "\\u2028";
  if (code === 8233) return "\\u2029";
  return char;
}

function encodeNumber(value: number): WireValue {
  if (Number.isFinite(value)) {
    return Object.is(value, -0) ? { tag: "negzero" } : { tag: "number", value };
  }
  if (Number.isNaN(value)) return { tag: "nan" };
  const sign: 1 | -1 = value > 0 ? 1 : -1;
  return { tag: "infinity", sign };
}

function isArrayIndexKey(key: string): boolean {
  if (key === "") return false;
  const index = Number(key);
  return Number.isInteger(index) && index >= 0 && index < 4294967295 && String(index) === key;
}

function describe(value: unknown): string {
  if (typeof value === "function") {
    return value.name !== "" ? `function ${value.name}` : "function";
  }
  if (typeof value === "symbol") return String(value);
  const inner = Object.prototype.toString.call(value).slice(8, -1);
  if (inner === "Module") return "module namespace";
  if (inner === "Object") {
    if (typeof value === "object" && value !== null && "constructor" in value) {
      const ctor = value.constructor;
      if (typeof ctor === "function" && ctor.name !== "" && ctor.name !== "Object") {
        return `instance of ${ctor.name}`;
      }
    }
    return "object with custom prototype";
  }
  return inner;
}

function formatPath(path: readonly (string | number)[]): string {
  let out = "$";
  for (const segment of path) {
    if (typeof segment === "number") {
      out += `[${segment}]`;
    } else if (/^[A-Za-z_$][A-Za-z0-9_$]*$/.test(segment)) {
      out += `.${segment}`;
    } else {
      out += `[${JSON.stringify(segment)}]`;
    }
  }
  return out;
}

function freezeNode(node: WireNode): WireNode {
  switch (node.kind) {
    case "array":
      for (const entry of node.items) {
        Object.freeze(entry[1]);
        Object.freeze(entry);
      }
      Object.freeze(node.items);
      for (const entry of node.extra) {
        Object.freeze(entry[1]);
        Object.freeze(entry);
      }
      Object.freeze(node.extra);
      break;
    case "record":
      for (const entry of node.entries) {
        Object.freeze(entry[1]);
        Object.freeze(entry);
      }
      Object.freeze(node.entries);
      break;
    case "date":
      Object.freeze(node.time);
      break;
    case "error":
      if (node.cause !== undefined) Object.freeze(node.cause);
      break;
  }
  return Object.freeze(node);
}

function malformed(where: string, detail: string): CodecError {
  return new CodecError(`reze: malformed hydration data at ${where}: ${detail}`);
}

function repr(value: unknown): string {
  try {
    const json = JSON.stringify(value);
    if (typeof json === "string") return json;
    return String(value);
  } catch {
    return "[unrepresentable]";
  }
}

function validateFrame(frame: unknown, frameIndex: number): GraphFrame {
  const where = `frame ${frameIndex}`;
  if (typeof frame !== "object" || frame === null || Array.isArray(frame)) {
    throw malformed(where, "frame must be an object");
  }
  if (!("nodes" in frame)) {
    throw malformed(where, "nodes must be an array");
  }
  const nodes: unknown = frame.nodes;
  if (!Array.isArray(nodes)) {
    throw malformed(where, "nodes must be an array");
  }
  const ids = new Set<number>();
  for (const node of nodes) {
    const id = nodeIdentity(node, where);
    if (ids.has(id)) throw malformed(where, `duplicate node ${id}`);
    ids.add(id);
  }
  for (const node of nodes) validateNodeBody(node as WireNode, where, ids);
  return frame as GraphFrame;
}

function nodeIdentity(node: unknown, where: string): number {
  if (typeof node !== "object" || node === null || Array.isArray(node)) {
    throw malformed(where, "node must be an object");
  }
  const id: unknown = "id" in node ? node.id : undefined;
  if (typeof id !== "number" || !Number.isInteger(id) || id < 0) {
    throw malformed(where, "node id must be a non-negative integer");
  }
  const kind: unknown = "kind" in node ? node.kind : undefined;
  if (kind !== "array" && kind !== "record" && kind !== "date" && kind !== "error") {
    throw malformed(where, `unknown node kind ${repr(kind)}`);
  }
  return id;
}

function validateNodeBody(node: WireNode, where: string, ids: Set<number>): void {
  const context = `${where} node ${node.id}`;
  switch (node.kind) {
    case "array": {
      const length: unknown = node.length;
      if (typeof length !== "number" || !Number.isInteger(length) || length < 0 || length > 4294967295) {
        throw malformed(context, "array length must be an integer between 0 and 4294967295");
      }
      const items: unknown = node.items;
      if (!Array.isArray(items)) throw malformed(context, "array items must be an array");
      const indices = new Set<number>();
      for (const entry of items) {
        if (!Array.isArray(entry) || entry.length !== 2) {
          throw malformed(context, "array item must be an [index, value] pair");
        }
        const index: unknown = entry[0];
        if (typeof index !== "number" || !Number.isInteger(index) || index < 0 || index >= length) {
          throw malformed(context, `array index ${repr(index)} is out of bounds`);
        }
        if (indices.has(index)) throw malformed(context, `duplicate array index ${index}`);
        indices.add(index);
        validateWireValue(entry[1], `${context} [${index}]`, ids);
      }
      const extra: unknown = node.extra;
      if (!Array.isArray(extra)) throw malformed(context, "array extra must be an array");
      const extraKeys = new Set<string>();
      for (const entry of extra) {
        if (!Array.isArray(entry) || entry.length !== 2) {
          throw malformed(context, "array extra entry must be a [key, value] pair");
        }
        const key: unknown = entry[0];
        if (typeof key !== "string") throw malformed(context, "array extra key must be a string");
        if (key === "length" || isArrayIndexKey(key)) {
          throw malformed(context, `array extra key ${JSON.stringify(key)} overlaps an index`);
        }
        if (extraKeys.has(key)) throw malformed(context, `duplicate array extra key ${JSON.stringify(key)}`);
        extraKeys.add(key);
        validateWireValue(entry[1], `${context} ${JSON.stringify(key)}`, ids);
      }
      return;
    }
    case "record": {
      const nullable: unknown = node.null;
      if (typeof nullable !== "boolean") throw malformed(context, "record null flag must be a boolean");
      const entries: unknown = node.entries;
      if (!Array.isArray(entries)) throw malformed(context, "record entries must be an array");
      const keys = new Set<string>();
      for (const entry of entries) {
        if (!Array.isArray(entry) || entry.length !== 2) {
          throw malformed(context, "record entry must be a [key, value] pair");
        }
        const key: unknown = entry[0];
        if (typeof key !== "string") throw malformed(context, "record key must be a string");
        if (keys.has(key)) throw malformed(context, `duplicate record key ${JSON.stringify(key)}`);
        keys.add(key);
        validateWireValue(entry[1], `${context} ${JSON.stringify(key)}`, ids);
      }
      return;
    }
    case "date": {
      validateDateTime(node.time, context);
      return;
    }
    case "error": {
      const native: unknown = node.native;
      if (typeof native !== "string" || !Object.hasOwn(ERROR_CONSTRUCTORS, native)) {
        throw malformed(context, "error native must be a known native error");
      }
      const name: unknown = node.name;
      if (typeof name !== "string") throw malformed(context, "error name must be a string");
      const message: unknown = node.message;
      if (typeof message !== "string") throw malformed(context, "error message must be a string");
      const hasCause: unknown = node.hasCause;
      if (typeof hasCause !== "boolean") throw malformed(context, "error hasCause must be a boolean");
      if (hasCause) {
        if (!Object.hasOwn(node, "cause")) throw malformed(context, "error declares a cause but carries none");
        validateWireValue(node.cause, `${context} cause`, ids);
      } else if (Object.hasOwn(node, "cause")) {
        throw malformed(context, "error carries a cause but declares none");
      }
      return;
    }
  }
}

function validateDateTime(time: unknown, where: string): void {
  if (typeof time !== "object" || time === null || Array.isArray(time)) {
    throw malformed(where, "date time must be a value");
  }
  const tag: unknown = "tag" in time ? time.tag : undefined;
  if (tag === "nan") return;
  if (tag === "number") {
    const millis: unknown = "value" in time ? time.value : undefined;
    if (typeof millis !== "number" || !Number.isFinite(millis) || Object.is(millis, -0)) {
      throw malformed(where, "date time must be a finite number");
    }
    return;
  }
  throw malformed(where, `date time has an unsupported tag ${repr(tag)}`);
}

function validateWireValue(value: unknown, where: string, ids: Set<number> | null): void {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw malformed(where, `value ${repr(value)} must be a tagged object`);
  }
  const tag: unknown = "tag" in value ? value.tag : undefined;
  switch (tag) {
    case "undefined":
    case "null":
    case "nan":
    case "negzero":
      return;
    case "boolean": {
      const flag: unknown = "value" in value ? value.value : undefined;
      if (typeof flag !== "boolean") throw malformed(where, "boolean value must be a boolean");
      return;
    }
    case "string": {
      const text: unknown = "value" in value ? value.value : undefined;
      if (typeof text !== "string") throw malformed(where, "string value must be a string");
      return;
    }
    case "number": {
      const num: unknown = "value" in value ? value.value : undefined;
      if (typeof num !== "number" || !Number.isFinite(num) || Object.is(num, -0)) {
        throw malformed(where, "number value must be a finite number other than -0");
      }
      return;
    }
    case "infinity": {
      const sign: unknown = "sign" in value ? value.sign : undefined;
      if (sign !== 1 && sign !== -1) throw malformed(where, "infinity sign must be 1 or -1");
      return;
    }
    case "bigint": {
      const digits: unknown = "value" in value ? value.value : undefined;
      if (typeof digits !== "string" || !/^-?\d+$/.test(digits)) {
        throw malformed(where, "bigint value must be a decimal integer string");
      }
      return;
    }
    case "ref": {
      const id: unknown = "id" in value ? value.id : undefined;
      if (typeof id !== "number" || !Number.isInteger(id) || id < 0) {
        throw malformed(where, "reference id must be a non-negative integer");
      }
      if (ids !== null && !ids.has(id)) {
        throw malformed(where, `reference to unknown node ${id}`);
      }
      return;
    }
    default:
      throw malformed(where, `unknown tag ${repr(tag)}`);
  }
}

function createShell(node: WireNode): unknown {
  switch (node.kind) {
    case "array":
      return [];
    case "record":
      return node.null ? Object.create(null) : {};
    case "date":
      return new Date(0);
    case "error": {
      const ctor = ERROR_CONSTRUCTORS[node.native];
      return new (ctor ?? Error)();
    }
  }
}

function fillNode(
  shell: unknown,
  node: WireNode,
  shells: Map<number, Materialized>,
  frameIndex: number,
): void {
  switch (node.kind) {
    case "array": {
      const arrayShell = shell as unknown[];
      const keep = new Set<number>();
      for (const [index] of node.items) keep.add(index);
      const extraKeep = new Set<string>();
      for (const [key] of node.extra) extraKeep.add(key);
      if (arrayShell.length !== node.length) arrayShell.length = node.length;
      for (const [index, item] of node.items) {
        defineData(arrayShell, index, resolveWire(item, shells, frameIndex));
      }
      for (const key of Object.keys(arrayShell)) {
        if (isArrayIndexKey(key)) {
          if (!keep.has(Number(key))) Reflect.deleteProperty(arrayShell, key);
        } else if (!extraKeep.has(key)) {
          Reflect.deleteProperty(arrayShell, key);
        }
      }
      for (const [key, item] of node.extra) {
        defineData(arrayShell, key, resolveWire(item, shells, frameIndex));
      }
      return;
    }
    case "record": {
      const recordShell = shell as Record<string, unknown>;
      if ((Object.getPrototypeOf(recordShell) === null) !== node.null) {
        Object.setPrototypeOf(recordShell, node.null ? null : Object.prototype);
      }
      const keep = new Set<string>();
      for (const [key] of node.entries) keep.add(key);
      for (const key of Object.keys(recordShell)) {
        if (!keep.has(key)) Reflect.deleteProperty(recordShell, key);
      }
      for (const [key, item] of node.entries) {
        defineData(recordShell, key, resolveWire(item, shells, frameIndex));
      }
      return;
    }
    case "date": {
      const dateShell = shell as Date;
      const time = node.time;
      if (time.tag !== "number" && time.tag !== "nan") {
        throw new CodecError("reze: malformed hydration data: date time must be a number");
      }
      dateShell.setTime(time.tag === "number" ? time.value : NaN);
      return;
    }
    case "error": {
      const errorShell = shell as Error;
      const ctor = ERROR_CONSTRUCTORS[node.native] ?? Error;
      if (Object.getPrototypeOf(errorShell) !== ctor.prototype) {
        Object.setPrototypeOf(errorShell, ctor.prototype);
      }
      defineSilent(errorShell, "message", node.message);
      const proto: unknown = Object.getPrototypeOf(errorShell);
      const protoName: unknown =
        typeof proto === "object" && proto !== null && "name" in proto ? proto.name : undefined;
      if (node.name === protoName) {
        if (Object.hasOwn(errorShell, "name")) Reflect.deleteProperty(errorShell, "name");
      } else {
        defineSilent(errorShell, "name", node.name);
      }
      if (node.hasCause) {
        const cause: unknown = node.cause;
        if (cause === undefined) {
          throw new CodecError(`reze: hydration node ${node.id} in frame ${frameIndex} is missing its cause`);
        }
        defineSilent(errorShell, "cause", resolveWire(cause as WireValue, shells, frameIndex));
      } else if (Object.hasOwn(errorShell, "cause")) {
        Reflect.deleteProperty(errorShell, "cause");
      }
      return;
    }
  }
}

function resolveWire(wire: WireValue, shells: Map<number, Materialized>, frameIndex: number): unknown {
  switch (wire.tag) {
    case "undefined":
      return undefined;
    case "null":
      return null;
    case "boolean":
    case "string":
    case "number":
      return wire.value;
    case "nan":
      return NaN;
    case "negzero":
      return -0;
    case "infinity":
      return wire.sign > 0 ? Infinity : -Infinity;
    case "bigint":
      return BigInt(wire.value);
    case "ref": {
      const shell = shells.get(wire.id);
      if (shell === undefined) {
        throw new CodecError(`reze: hydration reference ${wire.id} in frame ${frameIndex} has not been materialized`);
      }
      return shell.value;
    }
  }
}

function defineData(target: object, key: PropertyKey, value: unknown): void {
  Object.defineProperty(target, key, { value, writable: true, enumerable: true, configurable: true });
}

function defineSilent(target: object, key: PropertyKey, value: unknown): void {
  Object.defineProperty(target, key, { value, writable: true, enumerable: false, configurable: true });
}
