import type { InputHTMLAttributes } from "react";

import { cn } from "./cn";

export function Input({ className, ...props }: InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      className={cn("min-h-11 w-full rounded-xl border border-default bg-canvas px-3 text-base text-primary outline-none placeholder:text-muted focus:border-accent focus:ring-2 focus:ring-accent/20", className)}
      {...props}
    />
  );
}
