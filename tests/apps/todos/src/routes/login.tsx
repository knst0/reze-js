import { mutation, redirect } from "reze-js/server";

export const login = mutation(async (input: { name: string }, { response }) => {
  response.cookies.set("user", input.name, { secure: false });
  redirect("/todos");
});

export default function Login() {
  return (
    <main>
      <h1>Sign in</h1>
      <form action={login}>
        <label>
          Name <input name="name" />
        </label>
        <button type="submit">Sign in</button>
      </form>
    </main>
  );
}
