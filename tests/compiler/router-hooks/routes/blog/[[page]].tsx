export default function Blog(props: { params: { page?: string } }) {
  return <h1>Blog {props.params.page}</h1>;
}
