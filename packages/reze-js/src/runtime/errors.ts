/** A field the server rejected: its path in the input and the validator's message. */
export interface Issue {
  message: string;
  path: (string | number)[];
}

/** Thrown by `.call` in the browser when the server rejects the input. */
export class MutationInputError extends Error {
  constructor(readonly issues: Issue[]) {
    super("Invalid mutation input");
  }
}
