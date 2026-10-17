export const route = {
  preload: () => ({ go: true }),
  redirect: ({ data }: { data: { go: boolean } }) => (data.go ? { to: "/about" } : undefined),
};

export default function CondPage() {
  return <p>cond</p>;
}
