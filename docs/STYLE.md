# Documentation style

Operating guide for anyone, human or agent, who writes or edits a page under `docs/src/routes/`. It
says what to verify, how to write, and which commands prove the page is correct. Rules use MUST,
NEVER, and SHOULD in the RFC 2119 sense.

When this file is silent, follow the [Google developer documentation style guide](https://developers.google.com/style),
then [Merriam-Webster](https://www.merriam-webster.com/) for spelling.

## Contract

- The compiler and the code are the truth; the docs describe them. When the docs disagree with the
  code, the docs are wrong: fix the page. NEVER call the code's behavior a bug, a limitation, or a
  discrepancy in a page, and NEVER change code to match the docs.
- Every claim MUST match the code in this repository at the time you write it. A claim you have not
  verified against source or by running code MUST NOT ship.
- Every `ts` and `tsx` sample MUST pass `docs/plugins/examples.spec.ts`: it compiles for the
  `client`, `island`, and `html` targets with exactly the diagnostics its fence declares, and it
  typechecks. Mark incomplete code as a fragment instead of letting it fail.
- NEVER document an API from memory, from another framework, or from a test name. Read the export.
- NEVER document runtime internals: `@rezejs/signals`, `@rezejs/dom` compiler helpers, and
  `reze-js/internal/*` are a private ABI. Document what the user writes and what the compiler does
  with it.
- NEVER promise future behavior ("coming soon", "planned", "in a future release").
- One topic lives on one page. Other pages link to it instead of restating it.
- NEVER edit `packages/compiler/skills/reze-compiler-diagnostics/SKILL.md` by hand. The compiler
  generates it from its diagnostic catalog in `crates/reze_compiler/src/diagnostic/`. The
  "Diagnostic reference" tables on `compiler.mdx` MUST list exactly its codes;
  `docs/plugins/diagnostics.spec.ts` enforces that.

## Workflow

1. **Scope.** Find the owning page in the [page map](#page-map). If the topic belongs elsewhere, edit
   that page and link to it.
2. **Read the source.** Open the export, its types, and its tests through the
   [sources of truth](#sources-of-truth). Note exact names, option keys, defaults, and error cases.
3. **Prove behavior.** For each behavioral claim (ordering, timing, what updates, what throws), read
   the implementation or run a throwaway test, as described in [Behavior checks](#behavior-checks).
   For each "the compiler refuses or reports X" claim, write a sample with `expect=CODE` so the spec
   proves the diagnostic.
4. **Write.** Follow [Page structure](#page-structure), [Code samples](#code-samples), and
   [Prose](#prose).
5. **Validate.** Run every command in [Validation](#validation). All of them MUST pass.
6. **Clean up.** Delete throwaway tests and scratch files. Leave no generated output in the tree.

## Sources of truth

| Topic                                                   | Read                                                                                              |
| ------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| User-facing exports of `reze-js`                        | `packages/reze-js/src/index.ts` and the files it re-exports                                       |
| JSX attribute types, intrinsic elements                 | `packages/dom/src/jsx.ts`, `packages/dom/src/intrinsics.ts`, `packages/dom/src/jsx-properties.ts` |
| Signal, store, action, effect, owner, context semantics | `packages/signals/src/` and `packages/signals/tests/`                                             |
| DOM behavior: lists, portals, async views, islands      | `packages/dom/src/` and `packages/dom/tests/`                                                     |
| What the compiler accepts, rewrites, or refuses         | `crates/reze_compiler/src/` and `crates/reze_compiler/tests/`                                     |
| Diagnostic codes, severities, messages, and fixes       | `packages/compiler/skills/reze-compiler-diagnostics/SKILL.md`                                     |
| Direct compiler API                                     | `packages/compiler/index.d.ts`                                                                    |
| Router API, hooks, histories, typed paths               | `packages/router/src/index.ts` and `packages/router/tests/`                                       |
| File route naming and generated types                   | `packages/router/src/fs/`                                                                         |
| Vite plugin options, file routes, SSG                   | `packages/vite-plugin/src/` and `packages/vite-plugin/tests/`                                     |
| Runnable applications                                   | `examples/`                                                                                       |

Precedence, highest first: what the compiler accepts, refuses, and emits; the runtime code; the
declared types; doc comments and diagnostic message text; the current pages. Prior page text is a
list of topics to check, never evidence.

## Page map

Navigation lives in `docs/src/Shell.tsx`. If you rename a page title, update its `NavLink` label.
Keep these owners; a page outside its scope links to the owner.

| Page                                    | Owns                                                                                                                    |
| --------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| `installation.mdx` (Getting started)    | Project setup, first component, `render`, dev and build commands, setup troubleshooting                                 |
| `components.mdx` (Components and JSX)   | Component execution, props, children, prop helpers, supported JSX, `dynamic`                                            |
| `reactivity.mdx`                        | `signal`, `computed`, `store`, `action`: declaration rules, update semantics                                            |
| `lifecycle.mdx` (Lifecycle and cleanup) | `effect`, scheduling and `flush`, ownership and cleanup, context, error handling                                        |
| `familiar-patterns.mdx`                 | Patterns from other frameworks, what each costs in Reze, what to write instead                                          |
| `building-interfaces.mdx`               | Events, conditions, lists and `selector`, styling, forms and `createUniqueId`, refs, portals, async components, islands |
| `routing.mdx`                           | Router guide: file routes, layouts, params, typed paths, data, navigation, deployment                                   |
| `router-api.mdx`                        | Router reference: `createRouter`, histories, route tables, hooks, links, route typing                                   |
| `vite-plugin.mdx`                       | Plugin options, library manifests, file routes option, SSG, preprocessors, hosting                                      |
| `compiler.mdx`                          | What the compiler lowers, module boundaries, compiled bindings, direct API, diagnostics                                 |

## Code samples

### Fence meta

Every fence MUST have a language tag: `tsx`, `ts`, `sh`, `json`, `html`, `css`, or `text` for
program output. NEVER tag TypeScript as `js`: the spec would skip it. The spec compiles
and typechecks `ts` and `tsx` fences only. Add these tokens after the language to change that:

| Token                | Effect                                                                                                                                                                                                                                                               |
| -------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `fragment`           | Skipped by the spec. Use only for code that cannot be a module, such as one option or a snippet that needs undeclared names. The sentence before it MUST call it a fragment.                                                                                         |
| `file=<path>`        | Writes the sample to `<path>` inside the page's scratch project. Samples on the same page can import each other by relative path. Files under `src/routes/` get generated route types, so `paths`, `RouteConfigFor`, and `RoutePropsFor` typecheck as in a real app. |
| `expect=CODE[,CODE]` | The sample MUST produce exactly these diagnostic codes on every target. Use it to show a refused or reported shape. It is not typechecked.                                                                                                                           |

The following fence declares a route module, so the generated route types apply:

````md
```tsx file=src/routes/posts/[id].tsx

```
````

### Sample rules

- A sample MUST compile with zero diagnostics unless its fence has `expect=`. Info diagnostics count:
  a `signal` that nothing writes reports `SIGNAL_FOLDED`, so write it somewhere or use `const`.
- Prefer a complete module the reader can paste: imports, an exported component, and no undeclared
  names. Use `file=` instead of `fragment` when the only gap is a sibling module.
- Show one idea per sample. Cut code that does not serve the sentence before it.
- Let `vp fmt` lay out samples. Run `pnpm exec vp fmt docs` after editing; NEVER hand-wrap against it.
- Introduce each sample with a sentence. End it with a colon if the sample follows directly.
- Mark omitted code with a comment in the sample's language, such as `// Other routes omitted.`.
  NEVER use `...` or `…` as a placeholder.
- Use `pnpm` in shell samples.
- Use plausible names (`todos`, `saveTodo`, `count`), not `foo`, `bar`, or `x`.

### Behavior checks

Prove timing, ordering, and update claims by running them. Write a throwaway spec named after the
page, such as `zz-docs-reactivity.spec.tsx`, run it, then delete it:

| Claim about                                        | Put the spec in                                                                      | Run                                                                                            |
| -------------------------------------------------- | ------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------- |
| Components, JSX, signals, stores, actions, effects | `packages/dom/tests/`                                                                | `pnpm exec vp test run --project @rezejs/dom packages/dom/tests/zz-docs-<page>.spec.tsx`       |
| Router behavior                                    | `packages/router/tests/`                                                             | `pnpm exec vp test run --project @rezejs/router packages/router/tests/zz-docs-<page>.spec.tsx` |
| Compiler output or diagnostics                     | A scratch `.mts` script under `docs/` that calls `compile()` from `@rezejs/compiler` | `node docs/<script>.mts`                                                                       |

`@rezejs/testing-library` provides `mount`, `tick`, `settle`, and `cleanup`. The existing specs in
those folders show the patterns. These specs run through the Vite plugin, so compiler syntax works
in them.

## Validation

Run these from the repository root. Each MUST pass before you finish:

| Check                                    | Command                                                       | Pass condition                                                     |
| ---------------------------------------- | ------------------------------------------------------------- | ------------------------------------------------------------------ |
| Packages are current                     | `pnpm build`                                                  | Exit code 0. Run it first if you changed or pulled package source. |
| Samples, links, and diagnostic reference | `pnpm exec vp test run --project @rezejs/docs`                | All tests pass                                                     |
| Site builds                              | `pnpm --filter @rezejs/docs build`                            | Exit code 0                                                        |
| Formatting                               | `pnpm exec vp fmt docs`, then `pnpm exec vp fmt --check docs` | No changes reported                                                |

A failing sample test prints the compiler's rendered diagnostic or the TypeScript error with the
sample's page and line. Fix the sample or the claim. NEVER silence a failure by adding `fragment`
or `expect=` to a sample that should compile cleanly.

## Page structure

- Frontmatter MUST have `title` and `description`. The build fails without them. `description` is
  one sentence that says what the reader learns or does.
- One H1, equal to `title`. Don't skip heading levels.
- Open with one or two sentences that say what the page covers and what the reader can do after.
- Order sections from most common use to edge cases. Put rules the compiler enforces next to the
  feature they constrain, not in a separate list at the end.
- End with a "Related pages" section that links the neighboring owners.
- Headings use sentence case. A task heading starts with a bare infinitive ("Add a route"). A
  concept heading is a noun phrase ("Signal updates"). NEVER start a heading with an _-ing_ verb,
  put a link in a heading, or use code font alone in a heading: write "The `map` expression".
- Prefix a section not every reader needs with "Optional:".

## Prose

### Voice

- Address the reader as "you". The compiler, the runtime, or the router is the actor; never "we".
- Use active voice and present tense: "The compiler lowers `signal`", not "`signal` is lowered".
- Put conditions before instructions: "If `items` is empty, the list renders nothing."
- State the rule, then the consequence, then the fix. Skip motivation the reader did not ask for.
- NEVER use: "please", "simply", "easy", "just", "quickly", "let's", "note that", "at this time",
  exclamation marks, idioms, or marketing adjectives ("blazing", "powerful", "seamless").
- Write "lets you", not "allows you to". Write "earlier" or "later", not "above" or "below".

### Words

- American spelling. Serial comma.
- Spell out an abbreviation on first use per page only when the reader may not know it: "static site
  generation (SSG)". Don't expand JSX, DOM, HTML, CSS, or URL.
- Write "For more information about X, see [Y](/y)." when a link stands alone.
- Use "stop" or "cancel", not "abort", except for the `AbortSignal` API.
- Name list items; NEVER write "etc." or "and so on".

### Code in text

- Code font for identifiers, attributes, file names, paths, package names, commands, flags,
  environment variables, diagnostic codes, and literal values.
- No code font for product names (Reze, Vite, TypeScript) or URLs the reader opens.
- NEVER pluralize or possess a code element: "`computed` values", not "`computed`s".
- Code font for a Boolean only when you mean the value: "returns `true`", but "the condition is true".

### Lists, procedures, and links

- Numbered lists for sequences, bulleted lists for sets, tables for comparisons of two or more
  attributes.
- One action per numbered step. Put the goal and place first: "In `vite.config.ts`, add the plugin."
- Link text names the destination. NEVER use "click here", "this page", or a bare URL.
- Internal links are site-root-relative: `/routing#build-typed-paths`. `docs/plugins/links.spec.ts`
  fails on a link to a missing page or heading anchor; `rehype-slug` derives anchors from heading
  text, so renaming a heading breaks links to it.
- Link each destination once per page, at its first relevant mention.
- External links use HTTPS.

## Terminology

| Term                                                        | Form       | Notes                                                                                               |
| ----------------------------------------------------------- | ---------- | --------------------------------------------------------------------------------------------------- |
| Reze                                                        | Plain text | Product name. NEVER code font.                                                                      |
| `reze-js`                                                   | Code font  | The npm package applications import.                                                                |
| `@rezejs/vite-plugin`, `@rezejs/router`, `@rezejs/compiler` | Code font  | Packages a user installs or calls.                                                                  |
| `signal`, `computed`, `store`, `action`, `mergeProps`       | Code font  | Compiler syntax. Describe them as syntax the compiler lowers, not as functions with a runtime body. |
| `render`                                                    | Code font  | The value exports of `reze-js` that run as written.                                                 |
| compiler                                                    | Plain text | The Reze compiler.                                                                                  |
| component                                                   | Plain text | A function that returns JSX and runs once.                                                          |
| binding                                                     | Plain text | A compiled reactive update of one DOM attribute, property, or text node.                            |

## Done checklist

- [ ] Every claim traces to a file you read or a test you ran.
- [ ] `pnpm exec vp test run --project @rezejs/docs` passes.
- [ ] `pnpm --filter @rezejs/docs build` passes.
- [ ] `pnpm exec vp fmt --check docs` passes.
- [ ] No `fragment` or `expect=` hides a sample that should compile cleanly.
- [ ] The topic lives on its owning page; other pages link to it.
- [ ] Title and `description` are present; headings follow [Page structure](#page-structure).
- [ ] Throwaway specs and scratch files are deleted.
