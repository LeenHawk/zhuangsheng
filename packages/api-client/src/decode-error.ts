export class DecodeError extends Error {
  constructor(readonly path: string) {
    super(`Incompatible API response at ${path}`);
    this.name = "DecodeError";
  }
}
