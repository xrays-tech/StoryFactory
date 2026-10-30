import type { ReactNode } from "react";
import { cn } from "../../lib/cn";

export type BadgeTone = "neutral" | "accent" | "success" | "warning" | "danger" | "info" | "muted";

const tones: Record<BadgeTone, string> = {
  neutral: "border-line bg-line/25 text-muted",
  accent: "border-accent/25 bg-accent-soft text-accent-strong dark:text-accent",
  success: "border-success/20 bg-success-soft text-success",
  warning: "border-warning/20 bg-warning-soft text-warning",
  danger: "border-danger/20 bg-danger-soft text-danger",
  info: "border-info/20 bg-info-soft text-info",
  muted: "border-line/60 bg-transparent text-faint",
};

export type BadgeProps = {
  tone?: BadgeTone;
  children: ReactNode;
  className?: string;
  dot?: boolean;
  pulse?: boolean;
};

export function Badge({ tone = "neutral", children, className, dot, pulse }: BadgeProps) {
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1.5 rounded-full border px-2 py-0.5 text-[11px] font-medium leading-5",
        tones[tone],
        className,
      )}
    >
      {dot ? (
        <span
          className={cn("h-1.5 w-1.5 rounded-full bg-current", pulse && "animate-pulse-soft")}
          aria-hidden
        />
      ) : null}
      {children}
    </span>
  );
}
