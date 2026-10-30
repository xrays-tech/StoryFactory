import { Loader2 } from "lucide-react";
import { cn } from "../../lib/cn";

export function Skeleton({ className }: { className?: string }) {
  return <div className={cn("skeleton h-4 w-full", className)} aria-hidden />;
}

export function SkeletonLines({ rows = 3, className }: { rows?: number; className?: string }) {
  return (
    <div className={cn("space-y-2", className)} aria-hidden>
      {Array.from({ length: rows }).map((_, i) => (
        <Skeleton key={i} className={cn("h-3.5", i === rows - 1 ? "w-2/3" : "w-full")} />
      ))}
    </div>
  );
}

export function SkeletonCard({ className }: { className?: string }) {
  return (
    <div className={cn("surface-card space-y-4 p-5", className)} aria-hidden>
      <Skeleton className="h-3 w-20" />
      <Skeleton className="h-6 w-2/3" />
      <SkeletonLines rows={3} />
      <div className="flex gap-2">
        <Skeleton className="h-6 w-16 rounded-full" />
        <Skeleton className="h-6 w-20 rounded-full" />
      </div>
    </div>
  );
}

export function Spinner({ className, label = "正在加载" }: { className?: string; label?: string }) {
  return (
    <span className={cn("inline-flex items-center gap-2 text-sm text-muted", className)} role="status">
      <Loader2 className="h-4 w-4 animate-spin" aria-hidden />
      <span>{label}</span>
    </span>
  );
}

export function LoadingBlock({ label = "正在读取…" }: { label?: string }) {
  return (
    <div className="flex items-center justify-center rounded-2xl border border-line/70 bg-surface/40 py-16">
      <Spinner label={label} />
    </div>
  );
}
