# Build

Status: draft. How the Reze packages themselves are built for publishing. Rules use MUST, NEVER,
and SHOULD in the RFC 2119 sense.

## Mangle runtime properties

Application bundlers shorten imports and local names but keep property names. The runtime is a
private compiler target, so its internal property names are shortened before publishing.

Internal properties start with `_`:

```ts
interface Computation {
  _value: unknown;
  _sources: Computation[];
  _observers: Computation[];
}
```

The packages build with esbuild `mangleProps: /^_/`, so npm receives `a`, `b`, `c` in their place.
Public names never start with `_`.

One `mangleCache` file in the repository serves every package, so `signals` and `dom` shorten the
same property to the same name. A change of the cache is reviewed like any other source change.

Compiler output SHOULD NOT read runtime properties: the runtime exposes functions, and the
compiler calls them. Where emitted code must read a property, the compiler takes its name from the
same cache.

The size benchmark measures the effect. Mangling stays only while it saves bytes after gzip and
brotli.

## Implementation status

The current implementation differs from this design in these points:

- `reze-js` is the only package with a runtime. `vp pack` builds it, and `scripts/mangle.mts`
  then runs esbuild with `mangleProps: /^_/` over `dist`, with the cache in `scripts/mangle.json`.
- The mangled properties are the signal graph's, in `runtime/client.ts`. The size benchmark builds
  the test applications from `dist`, and mangling saves 31 bytes of gzip and 26 of brotli on each
  route chunk.
