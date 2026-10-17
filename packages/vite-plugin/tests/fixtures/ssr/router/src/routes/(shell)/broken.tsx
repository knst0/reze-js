export const route = {
  preload: () => {
    throw new Error("broken preload");
  },
};

export default function Broken() {
  return <h1 id="broken">unreachable</h1>;
}
