export default function Home() {
  const label = "Sketch";
  return (
    <main>
      <h1>{label}</h1>
      <canvas
        ref={(canvas) => {
          const context = canvas.getContext("2d");
          if (context) context.fillText(label, 0, 10);
        }}
      />
    </main>
  );
}
