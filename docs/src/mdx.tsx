import Check from "disarto-icons/icons/check-regular";
import Copy from "disarto-icons/icons/copy-regular";
import { signal } from "reze-js";
import type { JSX } from "reze-js";

type Props = { id?: string; children?: JSX.Element; [key: string]: unknown };
type MDXComponent = (props: Props) => JSX.Element;

const iconAttrs = { width: "16", height: "16", "aria-hidden": "true" };

function HeadingLink(props: { id?: string; children?: JSX.Element }) {
  return (
    <a href={`#${props.id}`} class="text-underline-offset-4 hover:underline">
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
      <pre class="overflow-x-auto rounded-3xl bg-bg-surface py-5 px-6 text-sm whitespace-pre" {...props} />
    </div>
  );
}

const components: Record<string, MDXComponent> = {
  h1: (props) => (
    <h1 id={props.id} class="mb-4 scroll-mt-8 text-3xl font-semibold">
      <HeadingLink id={props.id}>{props.children}</HeadingLink>
    </h1>
  ),
  h2: (props) => (
    <h2 id={props.id} class="mb-3 mt-10 scroll-mt-8 text-xl font-semibold">
      <HeadingLink id={props.id}>{props.children}</HeadingLink>
    </h2>
  ),
  h3: (props) => (
    <h3 id={props.id} class="mb-2 mt-8 scroll-mt-8 font-semibold">
      <HeadingLink id={props.id}>{props.children}</HeadingLink>
    </h3>
  ),
  h4: (props) => (
    <h4 id={props.id} class="mb-2 mt-6 scroll-mt-8 font-semibold">
      <HeadingLink id={props.id}>{props.children}</HeadingLink>
    </h4>
  ),
  p: (props) => <p class="mb-4" {...props} />,
  a: (props) => <a class="underline underline-offset-2" {...props} />,
  ul: (props) => <ul class="mb-4 list-disc space-y-1 pl-5" {...props} />,
  ol: (props) => <ol class="mb-4 list-decimal space-y-1 pl-5" {...props} />,
  li: (props) => <li {...props} />,
  em: (props) => <em {...props} />,
  strong: (props) => <strong class="font-semibold" {...props} />,
  del: (props) => <del class="line-through" {...props} />,
  code: (props) =>
    "data-sh-language" in props ? <code {...props} /> : <code class="rounded bg-accent/20 px-1 py-0.5 text-sm" {...props} />,
  pre: CodeBlock,
  table: (props) => <table class="mb-4 w-full text-left text-sm" {...props} />,
  thead: (props) => <thead {...props} />,
  tbody: (props) => <tbody {...props} />,
  tr: (props) => <tr {...props} />,
  th: (props) => <th class="border-b px-3 py-2 font-semibold" {...props} />,
  td: (props) => <td class="border-b px-3 py-2" {...props} />,
  blockquote: (props) => <blockquote class="my-4 border-l-2 pl-4" {...props} />,
  hr: (props) => <hr class="my-8" {...props} />,
  br: (props) => <br {...props} />,
  img: (props) => <img class="max-w-full rounded" {...props} />,
  input: (props) => <input class="mr-2" {...props} />,
  span: (props) => <span {...props} />,
};

export function useMDXComponents(extra: Record<string, MDXComponent> = {}): Record<string, MDXComponent> {
  return { ...components, ...extra };
}
