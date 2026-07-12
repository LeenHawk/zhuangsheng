export const createIdempotencyKey = (): string => {
  if (typeof crypto.randomUUID !== "function") {
    throw new Error("This browser cannot generate secure idempotency keys.");
  }
  return crypto.randomUUID();
};
