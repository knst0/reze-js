export const route = {
  preload: () => new Promise<never>(() => {}),
};

export default function Slow() {
  return <p>slow</p>;
}
