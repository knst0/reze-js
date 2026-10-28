import { signal } from "reze-js";

export default function DashboardHome() {
  let clicks = signal(0);
  return (
    <div>
      <p id="dashboard-home">home</p>
      <button id="dashboard-inc" type="button" onClick={() => (clicks += 1)}>
        {clicks}
      </button>
    </div>
  );
}
