export function rejectDuplicateKeys(text: string): void {
  type Frame = { type: "object"; keys: Set<string>; expectsKey: boolean } | { type: "array" };
  const stack: Frame[] = [];
  for (let index = 0; index < text.length; index += 1) {
    const character = text[index];
    if (character === '"') {
      const start = index;
      index += 1;
      while (index < text.length && text[index] !== '"') {
        if (text[index] === "\\") index += 1;
        index += 1;
      }
      if (index >= text.length) return;
      const frame = stack.at(-1);
      if (frame?.type === "object" && frame.expectsKey) {
        const key = JSON.parse(text.slice(start, index + 1)) as string;
        if (frame.keys.has(key)) throw new SyntaxError(`duplicate JSON key: ${key}`);
        frame.keys.add(key);
        frame.expectsKey = false;
      }
      continue;
    }
    if (character === "{") stack.push({ type: "object", keys: new Set(), expectsKey: true });
    else if (character === "[") stack.push({ type: "array" });
    else if (character === "}" || character === "]") stack.pop();
    else if (character === ",") {
      const frame = stack.at(-1);
      if (frame?.type === "object") frame.expectsKey = true;
    }
  }
}
