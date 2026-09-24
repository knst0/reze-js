import { Outlet } from "@rezejs/router";

export default function Users() {
  return (
    <>
      <h1>Users</h1>
      <ul>
        <li>
          <a href="/users/1">Ada</a>
        </li>
        <li>
          <a href="/users/2">Grace</a>
        </li>
      </ul>
      <Outlet />
    </>
  );
}
