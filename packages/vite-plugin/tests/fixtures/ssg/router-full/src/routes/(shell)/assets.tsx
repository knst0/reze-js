import { emittedLogo, inlineLogo, inlineMetaLogo, logoUrl, metaLogo } from "../../assets";

export default function AssetsPage() {
  return (
    <article>
      <img id="asset-img" src={logoUrl} alt="asset" />
      <img id="asset-inline" src={inlineLogo} alt="inline" />
      <img id="asset-emitted" src={emittedLogo} alt="emitted" />
      <img id="asset-meta" src={metaLogo} alt="meta" />
      <img id="asset-inline-meta" src={inlineMetaLogo} alt="inline meta" />
    </article>
  );
}
