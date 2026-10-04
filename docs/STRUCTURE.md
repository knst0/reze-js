# Documentation structure

Organize Reze documentation around learning, building, and looking up exact behavior. Package boundaries belong in API reference, not in the main learning path.

This document proposes the information architecture and authoring requirements. It is not a feature-support matrix. Before publishing a topic, verify its behavior against the implementation and a runnable example. Omit unsupported features from task-oriented navigation and record relevant limitations explicitly.

## Production build

Run `pnpm --filter @rezejs/docs build`. The Vite plugin enables `prerender: true`, which attempts to inline statically resolvable shells into `#app`; it does not generate HTML for every route. With the current `createRouter` entry in `src/main.tsx`, the built `#app` remains empty and the client renders the documentation. Enabling this option alone leaves the emitted HTML, JavaScript, CSS, and fonts byte-for-byte unchanged.

## MDX metadata and machine-readable documentation

Every published `.mdx` route starts with YAML frontmatter containing non-empty `title` and `description` strings:

```mdx
---
title: Getting started
description: Install Reze and build your first reactive component.
---

# Getting started
```

The MDX compiler exports this object as `frontmatter`; `src/vite-env.d.ts` declares its required fields. Frontmatter is not rendered as article content. Keep the article heading in the body.

`plugins/llms.ts` generates `/llms.txt` from these titles and descriptions, linking to one Markdown document per static MDX page: `/installation.md`, `/routing.md`, and so on. It uses the router's file scanner, including route groups and nested index routes; a root MDX index maps to `/index.md`. Private files and layout-only MDX routes are excluded. Dynamic MDX page routes fail generation because they have no single static URL. The current `/` redirect is TSX, so it has no Markdown document.

Markdown conversion uses the syntax tree, not source-text substitutions: YAML and MDX imports/exports are removed, JSX wrappers are unwrapped, and JavaScript expressions are omitted rather than executed. Write essential documentation as Markdown, not as output from an interactive component or expression. Code fences, tables, and other GFM content are preserved. Links to published documentation pages target their `.md` equivalents and retain query strings and fragments; external links are unchanged.

Both development and production use the same generator. Development serves fresh source on each GET or HEAD request; unknown `.md` URLs return 404. Production emits static files into `dist`, requiring no application server. Deploy these files before the host's SPA fallback. Vite's `base` prefixes generated links; relative-base deployments use root-relative document links. YAML parsing and Markdown generation run only in tooling, not in the browser.

Run `pnpm exec vitest run --project @rezejs/docs` for the build and HTTP contract tests.

## Navigation

The docs shell provides grouped navigation to all published pages, with the current page marked by `aria-current="page"`. On desktop the sidebar stays visible while the article scrolls; below 64rem it collapses behind the Menu button and closes when a link is selected. Keyboard users can skip directly to the article. Keep the links in `src/Shell.tsx` aligned with the published routes; planned topics below are not shown until their pages exist.

Each link group uses `src/hooks/useFluidHover.ts`, a Reze adaptation of [Fluid Hover](https://www.fluidfunctionalism.com/docs/fluid-hover). Call `useFluidHover()` during component setup, then pass its returned attachment function through a ref callback on a positioned container with static links and one `[data-fluid-hover-highlight]` element. Mouse movement picks the nearest row vertically, including gaps; an unmodified gap click activates that link. Headings separate groups, so highlights never travel across sections. The hook leaves native link clicks and keyboard focus alone, ignores touch hover, and releases listeners and its resize observer with the component owner. Measurements are cached until entry, resize, or scroll. CSS animates the highlight's transform; reduced motion disables travel while retaining the fade. The current-page marker remains independent of hover.

### Getting started

- **Getting started:** a short description of Reze followed by prerequisites, dependencies, complete Vite configuration, application entry point, development command, and production build command.
- **Your first component:** JSX, props, local reactive state, events, and composition in one runnable example.
- **Tutorial: reactive todo app:** a continuous path from a minimal application to adding, editing, filtering, and removing items.

The quick start at `/installation` is the documentation entry point. `/` redirects there with history replacement, and the brand links there directly. The sidebar provides the section index; do not duplicate it in an introduction page. The quick start must produce a working application without requiring readers to assemble configuration from other pages.

### Core concepts

- **Components and JSX:** component execution, supported JSX, bindings, and composition.
- **Reactive state:** declarations, tracked reads, assignments, update behavior, and syntax restrictions.
- **Computed values:** dependency tracking, evaluation, invalidation, and derived state.
- **Actions and effects:** intended use, execution timing, dependency behavior, and cleanup.
- **Stores:** supported data shapes, reads, updates, and identity semantics.
- **Props and component composition:** reactive props, children, and passing values between components.
- **Ownership and cleanup:** resource lifetime, disposal, and behavior when UI is removed.
- **How compilation changes JavaScript:** compile-time syntax, transformation boundaries, supported locations, and interaction with ordinary JavaScript.

Explain the execution model directly rather than relying on comparisons with other frameworks. Readers must know what runs once, what runs again, and what triggers an update.

### Building interfaces

- Events and user input.
- Conditional rendering.
- Lists and identity.
- Forms.
- Styling.
- DOM access.
- Context.
- Portals.
- Async data and loading states.

Each page answers a concrete UI-building question. Include state transitions, lifetime concerns, and relevant edge cases. Distinguish framework APIs from patterns implemented with browser APIs or application code.

### Routing

- Router setup.
- File-system routes.
- Navigation.
- Route parameters and search parameters.
- Layouts and nested routes.

Use a consistent example application throughout. Show route file trees alongside matching URLs and rendering behavior. Document URL parsing, unmatched routes, and navigation failures where supported. Do not imply routing capabilities that have not been verified.

### Guides

- **Structuring an application:** component, state, route, and shared-module organization.
- **Testing:** supported setup and testing observable application behavior.
- **Debugging and compiler diagnostics:** symptoms, diagnostic meaning, causes, and corrections.
- **Production builds and deployment:** build output, hosting requirements, and router-related server configuration.
- **Performance:** recommended patterns, measurement methods, and reproducible benchmarks.
- **Interoperability:** integration with ordinary JavaScript and third-party libraries, including ownership and cleanup boundaries.

Guides solve multi-step problems. Present one recommended approach first, then alternatives only where there is a meaningful tradeoff.

### API reference

- `reze-js` public entry point.
- `@rezejs/signals`.
- `@rezejs/router`.
- Vite plugin options.
- Compiler configuration and diagnostics.
- Low-level DOM APIs.

Clearly distinguish supported application APIs from compiler targets and internal implementation details. Low-level APIs must not appear necessary for routine application development.

Give each contract one authoritative home. Concept pages explain the model and link to reference rather than duplicating complete signatures and option tables.

### Examples

- Counter.
- Async todos.
- Routed application.

Prefer the repository's existing examples as runnable sources. Each example page includes launch instructions, the behavior to observe, and links to the concepts it demonstrates. Avoid maintaining a second complete copy of the application in prose.

### Project

- Supported features and limitations.
- Release notes and migration guides.
- Contributing.
- Architecture and compiler internals.

Keep application-development guidance separate from compiler-contributor material. Architecture documentation should explain compilation phases, runtime boundaries, and the principle of doing statically decidable work at compile time.

## Page requirements

| Page type | Reader's question | Required content |
| --- | --- | --- |
| Quick start | Can I run this? | Prerequisites, exact setup, complete files, commands, expected result, next step. |
| Tutorial | Can I learn by building? | Starting state, sequential changes, runnable checkpoints, observable results, completed source. |
| Concept | How does this work? | Mental model, minimal example, semantics, constraints, common mistakes. |
| UI task | How do I build this interface? | Working implementation, state transitions, lifecycle, relevant edge cases. |
| Routing | How do I connect screens? | File tree, configuration, URL examples, navigation behavior, failure cases. |
| Guide | How do I solve this broader problem? | Recommended approach, runnable implementation, tradeoffs, troubleshooting. |
| API reference | What does this API guarantee? | Import path, signature, inputs, return value, behavior, errors, lifecycle, example. |
| Example | What does a complete application look like? | Runnable source, launch instructions, expected behavior, important decisions. |
| Project | Can I adopt or contribute to this? | Stability, limitations, compatibility, release process, contribution workflow, architecture. |

### Default feature-page shape

1. **Purpose:** the problem and when to use the feature.
2. **Minimal working example:** include imports and necessary surrounding code.
3. **Behavior:** precise execution and update semantics.
4. **Common patterns:** realistic variations without duplicating the basic example.
5. **Constraints and pitfalls:** invalid usage, its consequence, and the supported alternative.
6. **Related APIs and next steps:** links to reference and the next useful topic.

Omit sections that add no information. Tutorials should read as a continuous sequence; reference pages should prioritize contracts over narrative.

## Reze-specific contracts to explain

### Compiler syntax and reactivity

The quick start uses `$signal` to make an assignable variable reactive. The documentation must establish:

- Where reactive declarations are allowed and which tooling transforms them.
- Which reads establish dependencies and in which execution contexts.
- Which assignments notify consumers and when updates become observable.
- What happens when reactive values cross function and module boundaries.
- How destructuring and passing values affect reactivity.
- What happens in files that are not transformed by the compiler.

Use supported and unsupported examples with explicit outcomes. Do not infer behavior from syntax resembling ordinary JavaScript.

### Execution and lifetime

Document when component bodies, computed values, and effects execute. Explain dependency tracking, ownership, and disposal, including what happens when conditional or list-rendered content is removed.

### Public and generated-code boundaries

Identify the intended import path for application authors. Mark compiler targets separately, and do not present internal helpers as stable public contracts without an explicit support commitment.

### Limitations

Maintain a central support overview, but also place restrictions next to the affected feature. A reader should not need to discover a separate limitations page to use an API correctly.

### Performance evidence

Separate architectural properties from measured results. Performance claims must identify benchmark source, commands, environment, build mode, and comparison conditions. Avoid unqualified claims about bundle size, memory, or speed.

## Writing order

1. Complete installation and Vite quick start.
2. Reactive-state and compiler-semantics pages.
3. Components, props, events, conditional rendering, and lists.
4. Ownership, effects, and cleanup.
5. Router setup and a runnable navigation example.
6. Public API reference.
7. Broader guides, advanced topics, and compiler internals.

This order establishes a working application and a correct mental model before expanding coverage.

## Publication checklist

- The page describes verified, supported behavior or explicitly labels a limitation.
- Commands and examples run against the documented version.
- Examples include necessary imports and setup, with no unexplained missing code.
- Observable results are stated, not merely successful compilation.
- API signatures and import paths agree with the implementation.
- Lifecycle rules and relevant failure cases are documented.
- Links point to existing pages or source files.
- Contracts have one authoritative location; related pages link to it.
- Changes to public behavior update the affected pages and release notes together.
