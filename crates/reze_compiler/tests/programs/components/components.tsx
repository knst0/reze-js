import { Show, signal } from "reze-js";
import { Widget } from "ui-kit";
import { brand, counter, links, year } from "./data";

export function Header(props) {
  return (
    <header title={props.title}>
      <b>{brand()}</b>
      <nav>{links.map((link) => <a href={link.href}>{link.label}</a>)}</nav>
    </header>
  );
}

export const Footer = ({ note }) => <footer>{note} © {year}</footer>;

export function Tree(props) {
  const depth = props.depth;
  return <ul>{depth > 0 ? <Tree depth={depth - 1} /> : null}</ul>;
}

export const Clicker = () => <button onClick={() => alert(1)}>click</button>;
export const WithRef = () => <div ref={(el) => el.focus()} />;
export const WithSpread = (props) => <div {...props} />;
export const WithProp = () => <input prop:indeterminate={true} />;
export const Select = () => <select value="b"><option value="b" /></select>;
export async function Async() { return <p />; }
export const TwoParams = (a, b) => <p>{a}{b}</p>;
export const Local = () => { let x = 1; return <p>{x}</p>; };
export const RuntimeChild = () => <Show when={true}><p /></Show>;
export const OutsideChild = () => <Widget />;
export const Deep = (props) => <p>{props.a.b}</p>;
export const Reactive = () => { const [n] = signal(0); return <p>{n()}</p>; };
export const Mutated = () => <p>{counter.value}</p>;
export const ClientChild = () => <section><Clicker /></section>;
