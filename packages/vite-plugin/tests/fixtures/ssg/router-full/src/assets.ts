import logoUrl from "./logo.svg?url";
import inlineLogo from "./logo.svg?inline";
import emittedLogo from "./logo.svg?no-inline";
import "./styles.css";

export { logoUrl, inlineLogo, emittedLogo };
export const metaLogo = new URL("./logo.svg?no-inline", import.meta.url).href;
export const inlineMetaLogo = new URL("./logo.svg?inline", import.meta.url).href;
