import type { JSX } from "reze-js/jsx-runtime";

import { fetchMotto, fetchUser, type User } from "./api";

export async function UserCard(props: { id: number }): Promise<JSX.Element> {
  const user: User = await fetchUser(props.id);
  const motto = await fetchMotto(user.id);
  const initials = user.name
    .split(" ")
    .map((part) => part[0])
    .join("");
  return (
    <article class="card">
      <span class="avatar">{initials}</span>
      <div>
        <h2>{user.name}</h2>
        <p>
          #{user.id} · {user.role}
        </p>
        <p class="motto">{motto}</p>
      </div>
    </article>
  );
}
