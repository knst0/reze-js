export interface Todo {
  id: number;
  title: string;
  priority: 1 | 2 | 3;
  done: boolean;
}

const lists = new Map<string, Todo[]>();

export function todosOf(user: string): Todo[] {
  let list = lists.get(user);
  if (!list) lists.set(user, (list = []));
  return list;
}

let renders = 0;

/** Counts renders, so a test tells a prerendered page from one rendered on request. */
export function render(): number {
  return ++renders;
}
