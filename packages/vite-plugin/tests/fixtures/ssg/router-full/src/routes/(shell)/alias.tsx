import type { RouteProps } from "@rezejs/router";
import { asyncComponent } from "reze-js/internal/runtime";

export const route = {
  preload: () => ({ items: ["m"] }),
};

interface AliasData {
  items: string[];
}

function AliasView(props: { data: AliasData }) {
  return asyncComponent(
    async (): Promise<[AliasData, AliasData]> => {
      const first = await Promise.resolve(props.data);
      first.items.push("n");
      const second = await Promise.resolve(props.data);
      return [first, second];
    },
    (values) => {
      const [first, second] = values();
      return (
        <p id="alias">
          {first === second ? "same" : "diff"}:{second.items.join(",")}
        </p>
      );
    },
  );
}

export default function AliasPage(props: RouteProps<Record<string, string>, { items: string[] }>) {
  return <AliasView data={props.data} />;
}
