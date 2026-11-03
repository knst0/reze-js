import { createApp, type Handler, type HandlerOptions, type Mutation, type Submission } from "./runtime/server";

export {
  defer,
  fail,
  guardRoute,
  loader,
  mutation,
  MutationInputError,
  notFound,
  redirect,
  RouteError,
  type CookieOptions,
  type GuardArgs,
  type Handler,
  type HandlerOptions,
  type Issue,
  type Loader,
  type LoaderArgs,
  type Mutation,
  type MutationArgs,
  type Register,
  type ResponseWriter,
  type StandardSchemaV1,
  type Submission,
} from "./runtime/server";

/** Compiler syntax: the current request's submission of `mutation`, `{ input, issues, result }`. */
export declare function useSubmission<I, R>(mutation: Mutation<I, R>): Submission<I, R>;

/** The request handler of the app, for any server that speaks `Request` and `Response`. */
export function createHandler(options: HandlerOptions = {}): Handler {
  const app = import("virtual:rz-app").then((m) =>
    createApp(m.default, m.root, { ...options, assets: m.assets, build: m.build, dev: m.dev }),
  );
  return async (request) => (await app)(request);
}
