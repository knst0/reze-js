import type { JSX } from "reze-js";

type Props = { children?: JSX.Element; [key: string]: unknown };
type MDXComponent = (props: Props) => JSX.Element;

const components: Record<string, MDXComponent> = {
  h1: (props) => <h1 class="mb-4 scroll-mt-8 text-3xl font-semibold" {...props} />,
  h2: (props) => <h2 class="mb-3 mt-10 scroll-mt-8 text-xl font-semibold" {...props} />,
  h3: (props) => <h3 class="mb-2 mt-8 scroll-mt-8 font-semibold" {...props} />,
  h4: (props) => <h4 class="mb-2 mt-6 scroll-mt-8 font-semibold" {...props} />,
  p: (props) => <p class="mb-4" {...props} />,
  a: (props) => <a class="underline underline-offset-2" {...props} />,
  ul: (props) => <ul class="mb-4 list-disc space-y-1 pl-5" {...props} />,
  ol: (props) => <ol class="mb-4 list-decimal space-y-1 pl-5" {...props} />,
  li: (props) => <li {...props} />,
  em: (props) => <em {...props} />,
  strong: (props) => <strong class="font-semibold" {...props} />,
  del: (props) => <del class="line-through" {...props} />,
  code: (props) => ("data-sh-language" in props ? <code {...props} /> : <code class="rounded bg-accent/20 px-1 py-0.5 text-sm" {...props} />),
  pre: (props) => <pre class="mb-4 overflow-x-auto rounded-3xl bg-bg-surface py-5 px-6 text-sm whitespace-pre" {...props} />,
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
