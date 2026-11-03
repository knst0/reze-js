function Button(props: { variant: string; type?: string; disabled?: boolean; children?: unknown }) {
  const { variant, children, ...rest } = props;
  return (
    <button class={`btn btn-${variant}`} {...rest}>
      {children}
    </button>
  );
}

function Label({ text = "none", ...rest }: { text?: string; id?: string }) {
  return <label {...rest}>{text}</label>;
}

export default function Home() {
  return (
    <form>
      <Label id="name" />
      <Label text="Email" />
      <Button variant="primary" type="submit" disabled>
        Save
      </Button>
    </form>
  );
}
