import { Slot } from "@radix-ui/react-slot";
import { cva, type VariantProps } from "class-variance-authority";
import type { ButtonHTMLAttributes } from "react";

import { cn } from "./cn";

const buttonVariants = cva(
  "inline-flex min-h-10 items-center justify-center gap-2 rounded-xl px-4 text-sm font-semibold transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-focus focus-visible:ring-offset-2 focus-visible:ring-offset-canvas disabled:pointer-events-none disabled:opacity-50",
  {
    variants: {
      variant: {
        primary: "bg-accent text-accent-contrast hover:bg-accent-strong",
        secondary: "border border-default bg-surface text-primary hover:bg-elevated",
        ghost: "text-secondary hover:bg-elevated hover:text-primary",
        danger: "bg-danger text-white hover:opacity-90",
      },
      size: {
        default: "min-h-10",
        compact: "min-h-8 rounded-lg px-3 text-xs",
        icon: "size-10 p-0",
      },
    },
    defaultVariants: { variant: "primary", size: "default" },
  },
);

export interface ButtonProps
  extends ButtonHTMLAttributes<HTMLButtonElement>, VariantProps<typeof buttonVariants> {
  asChild?: boolean;
}

export function Button({ asChild, className, variant, size, ...props }: ButtonProps) {
  const Component = asChild ? Slot : "button";
  return <Component className={cn(buttonVariants({ variant, size }), className)} {...props} />;
}
