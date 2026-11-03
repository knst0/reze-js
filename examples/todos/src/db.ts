export interface Todo {
  id: number;
  title: string;
  done: boolean;
}

const todos: Todo[] = [
  { id: 1, title: "Read the specs", done: true },
  { id: 2, title: "Build an app", done: false },
];

export const db = {
  all: async (): Promise<Todo[]> => todos,
  get: async (id: number): Promise<Todo | undefined> => todos.find((todo) => todo.id === id),
  add: async (title: string): Promise<Todo> => {
    const todo = { id: Math.max(0, ...todos.map((todo) => todo.id)) + 1, title, done: false };
    todos.push(todo);
    return todo;
  },
  toggle: async (id: number): Promise<void> => {
    const todo = todos.find((todo) => todo.id === id);
    if (todo) todo.done = !todo.done;
  },
  remove: async (id: number): Promise<void> => {
    const index = todos.findIndex((todo) => todo.id === id);
    if (index >= 0) todos.splice(index, 1);
  },
};
