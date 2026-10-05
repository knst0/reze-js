import type { JSX } from "reze-js";

type Props = { children?: JSX.Element; [key: string]: unknown };
type Component = (props: Props) => JSX.Element;

const components: Record<string, Component> = {
  h1: (props) => <h1 {...props} />,
  p: (props) => <p {...props} />,
  em: (props) => <em {...props} />,
  img: (props) => <img {...props} />,
};

export function useMDXComponents(extra: Record<string, Component> = {}): Record<string, Component> {
  return { ...components, ...extra };
}
