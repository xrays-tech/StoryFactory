import { cn } from "../../lib/cn";

export function ProgressBar({
  value,
  max,
  className,
  tone = "accent",
  label,
  valueText,
}: {
  value: number;
  max: number;
  className?: string;
  tone?: "accent" | "success" | "danger" | "info";
  label?: string;
  valueText?: string;
}) {
  const ratio = max > 0 ? Math.max(0, Math.min(1, value / max)) : 0;
  const tones = {
    accent: "bg-accent",
    success: "bg-success",
    danger: "bg-danger",
    info: "bg-info",
  };
  return (
    <div
      className={cn("h-1.5 w-full overflow-hidden rounded-full bg-line/60", className)}
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={max}
      aria-valuenow={value}
      aria-label={label}
      aria-valuetext={valueText}
    >
      <div
        className={cn("h-full rounded-full transition-[width] duration-200", tones[tone])}
        style={{ width: `${ratio * 100}%` }}
      />
    </div>
  );
}
