import type { Config } from "tailwindcss";

export default {
  content: [
    "./index.html",
    "./src/**/*.{ts,tsx}",
    "../../packages/ui/src/**/*.{ts,tsx}",
    "../../packages/domain-ui/src/**/*.{ts,tsx}",
  ],
  theme: {
    extend: {
      colors: {
        canvas: "hsl(var(--canvas))", surface: "hsl(var(--surface))", elevated: "hsl(var(--elevated))",
        primary: "hsl(var(--text-primary))", secondary: "hsl(var(--text-secondary))", muted: "hsl(var(--text-muted))",
        default: "hsl(var(--border))", accent: "hsl(var(--accent))", "accent-strong": "hsl(var(--accent-strong))",
        "accent-soft": "hsl(var(--accent-soft))", "accent-contrast": "hsl(var(--accent-contrast))",
        info: "hsl(var(--info))", running: "hsl(var(--running))", success: "hsl(var(--success))",
        warning: "hsl(var(--warning))", danger: "hsl(var(--danger))", focus: "hsl(var(--focus))",
      },
      backgroundImage: { hero: "linear-gradient(135deg, hsl(var(--surface)) 0%, hsl(var(--accent-soft)) 150%)" },
      boxShadow: { soft: "0 1px 2px hsl(220 25% 10% / .04)", panel: "0 24px 70px hsl(220 30% 8% / .10)", glow: "0 10px 30px hsl(var(--accent) / .22)" },
      fontFamily: { display: ["ui-serif", "Georgia", "Noto Serif SC", "serif"] },
    },
  },
  plugins: [],
} satisfies Config;
