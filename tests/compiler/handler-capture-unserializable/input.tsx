export default function Live() {
  const socket = new WebSocket("/live");
  return <button onClick={() => socket.send("ping")}>Ping</button>;
}
