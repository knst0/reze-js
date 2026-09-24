export interface User {
  id: number;
  name: string;
  role: string;
}

const DB: Record<number, User> = {
  1: { id: 1, name: "Ada Lovelace", role: "Analytical Engine" },
  2: { id: 2, name: "Grace Hopper", role: "COBOL & Compilers" },
  3: { id: 3, name: "Edsger Dijkstra", role: "Shortest Paths" },
};

const LATENCY: Record<number, number> = { 1: 900, 2: 150, 3: 500 };

export function fetchUser(id: number): Promise<User> {
  return new Promise((resolve) => {
    setTimeout(() => resolve(DB[id] ?? { id, name: `User ${id}`, role: "Unknown" }), LATENCY[id] ?? 400);
  });
}

const MOTTO: Record<number, string> = {
  1: "The Analytical Engine weaves algebraical patterns.",
  2: "Humans are allergic to change; bugs love it.",
  3: "Simplicity is prerequisite for reliability.",
};

export function fetchMotto(id: number): Promise<string> {
  return new Promise((resolve) => {
    setTimeout(() => resolve(MOTTO[id] ?? "Hello, world."), 300);
  });
}
