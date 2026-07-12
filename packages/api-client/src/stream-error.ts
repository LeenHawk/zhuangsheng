export class RunStreamProtocolError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "RunStreamProtocolError";
  }
}
