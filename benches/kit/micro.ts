export interface BenchLike {
  run(): Promise<unknown>;
  table(): unknown;
}

export interface GraphOptions {
  beforeAll(): void;
  afterAll(): void;
}

type RootFn = <T>(fn: (dispose: () => void) => T) => T;

export function withGraph(root: RootFn, build: () => void): GraphOptions {
  let dispose: (() => void) | undefined;
  return {
    beforeAll() {
      root((disposer) => {
        dispose = disposer;
        build();
      });
    },
    afterAll() {
      dispose?.();
    },
  };
}

export async function finishBench(bench: BenchLike): Promise<void> {
  await bench.run();
  console.table(bench.table());
}
