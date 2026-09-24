export async function User(props: { id: number }) {
  const user = await fetchUser(props.id);
  const posts = await fetchPosts(user.id);
  return (
    <ul>
      {posts.map((p) => (
        <li>{p.title}</li>
      ))}
    </ul>
  );
}

declare function fetchUser(id: number): Promise<{ id: number }>;
declare function fetchPosts(id: number): Promise<{ title: string }[]>;
