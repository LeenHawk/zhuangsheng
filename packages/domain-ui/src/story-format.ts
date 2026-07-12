export const shortId = (value: string) =>
  value.length > 18 ? `${value.slice(0, 10)}…${value.slice(-5)}` : value;
