export default function Home() {
  const user = { name: "Ann" };
  const count = 1;
  return (
    <main>
      {user ? <p>{user.name}</p> : <p>Sign in</p>}
      {count && <span>{count}</span>}
    </main>
  );
}
