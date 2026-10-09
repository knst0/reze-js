export type RequestHandler = (request: Request) => Promise<Response>;

export interface ServeOptions {
  port?: number;
  hostname?: string;
}

export interface RunningServer {
  readonly hostname: string;
  readonly port: number;
  close(): Promise<void>;
}

export const DefaultPort = 3000;

export async function guardedFetch(handler: RequestHandler, request: Request): Promise<Response> {
  try {
    return await handler(request);
  } catch (error) {
    console.error(error);
    return new Response("Internal Server Error", { status: 500 });
  }
}
