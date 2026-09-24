import { Outlet } from "../../../src";

export default function UsersLayout() {
  return (
    <section>
      <h1>users</h1>
      <Outlet />
    </section>
  );
}
