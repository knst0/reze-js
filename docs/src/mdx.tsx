import Check from "disarto-icons/icons/check-regular";
import Copy from "disarto-icons/icons/copy-regular";
import { signal } from "reze-js";
import type { JSX } from "reze-js";

type Props = { id?: string; children?: JSX.Element; [key: string]: unknown };
type MDXComponent = (props: Props) => JSX.Element;

const iconAttrs = { width: "16", height: "16", "aria-hidden": "true" };

function HeadingLink(props: { id?: string; children?: JSX.Element }) {
  return (
    <a href={`#${props.id}`} class="underline-offset-4 decoration-accent/40 hover:underline">
      {props.children}
    </a>
  );
}

function CodeBlock(props: Props) {
  let copied = signal(false);

  return (
    <div class="group relative mb-4">
      <button
        type="button"
        aria-label="Copy code"
        data-copied={copied ? "" : null}
        class="absolute top-3 right-3 z-10 size-8 btn border-none bg-bg-surface hover:bg-bg-element opacity-0 transition-opacity duration-150 group-hover:opacity-100 focus-visible:opacity-100 data-[copied]:opacity-100"
        onClick={(event: MouseEvent) => {
          const button = event.currentTarget;
          if (!(button instanceof HTMLElement)) return;
          const code = button.parentElement?.querySelector("pre")?.textContent ?? "";
          navigator.clipboard.writeText(code).then(() => {
            copied = true;
            setTimeout(() => {
              copied = false;
            }, 2000);
          });
        }}
      >
        {copied ? (
          <span class="grid animate-pop place-items-center" innerHTML={Check.toSvg(iconAttrs)} />
        ) : (
          <span class="grid animate-pop place-items-center" innerHTML={Copy.toSvg(iconAttrs)} />
        )}
      </button>
      <pre class="overflow-x-auto rounded-2xl bg-bg-surface py-5 px-6 text-sm leading-relaxed whitespace-pre" {...props} />
    </div>
  );
}

const components: Record<string, MDXComponent> = {
  h1: (props) => (
    <h1 id={props.id} class="mb-5 scroll-mt-8 text-4xl font-semibold tracking-tight text-fg-strong">
      <HeadingLink id={props.id}>{props.children}</HeadingLink>
    </h1>
  ),
  h2: (props) => (
    <h2 id={props.id} class="mb-3 mt-14 scroll-mt-8 text-2xl font-semibold tracking-tight text-fg-strong">
      <HeadingLink id={props.id}>{props.children}</HeadingLink>
    </h2>
  ),
  h3: (props) => (
    <h3 id={props.id} class="mb-2 mt-10 scroll-mt-8 text-lg font-semibold tracking-tight text-fg-strong">
      <HeadingLink id={props.id}>{props.children}</HeadingLink>
    </h3>
  ),
  h4: (props) => (
    <h4 id={props.id} class="mb-2 mt-8 scroll-mt-8 font-semibold text-fg-strong">
      <HeadingLink id={props.id}>{props.children}</HeadingLink>
    </h4>
  ),
  p: (props) => <p class="mb-4 leading-7" {...props} />,
  a: (props) => (
    <a
      class="font-medium text-fg-strong underline decoration-accent/40 underline-offset-[3px] transition-colors hover:decoration-accent"
      {...props}
    />
  ),
  ul: (props) => <ul class="mb-4 list-disc space-y-1.5 pl-5 leading-7 marker:text-fg-muted" {...props} />,
  ol: (props) => <ol class="mb-4 list-decimal space-y-1.5 pl-5 leading-7 marker:text-fg-muted" {...props} />,
  li: (props) => <li {...props} />,
  em: (props) => <em {...props} />,
  strong: (props) => <strong class="font-semibold" {...props} />,
  del: (props) => <del class="line-through" {...props} />,
  code: (props) =>
    "data-sh-language" in props ? (
      <code {...props} />
    ) : (
      <code class="rounded-md border border-border bg-bg-surface px-1.5 py-0.5 text-[0.85em]" {...props} />
    ),
  pre: CodeBlock,
  table: (props) => (
    <div class="mb-6 overflow-x-auto">
      <table class="w-full text-left text-sm tabular-nums" {...props} />
    </div>
  ),
  thead: (props) => <thead {...props} />,
  tbody: (props) => <tbody {...props} />,
  tr: (props) => <tr {...props} />,
  th: (props) => <th class="border-b border-bg-element px-3 py-2 font-semibold text-fg-strong" {...props} />,
  td: (props) => <td class="border-b border-border px-3 py-2 align-top" {...props} />,
  blockquote: (props) => <blockquote class="my-6 border-l-2 border-accent/40 pl-4 text-fg-muted" {...props} />,
  hr: (props) => <hr class="my-10 border-border" {...props} />,
  br: (props) => <br {...props} />,
  img: (props) => <img class="max-w-full rounded" {...props} />,
  input: (props) => <input class="mr-2" {...props} />,
  span: (props) => <span {...props} />,
};

export function useMDXComponents(extra: Record<string, MDXComponent> = {}): Record<string, MDXComponent> {
  return { ...components, ...extra };
}
