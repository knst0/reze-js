import { paths } from "virtual:reze-routes";

export default function Posts() {
  return (
    <ul>
      <li>
        <a href={paths.blog.byId(1)}>Post 1</a>
      </li>
      <li>
        <a href={paths.blog.byId(2)}>Post 2</a>
      </li>
    </ul>
  );
}
