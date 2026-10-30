import type { ReactNode } from "react";
import { AlertCircle, RefreshCw } from "lucide-react";
import { cn } from "../../lib/cn";
import { Button } from "./Button";

export type EmptyStateProps = {
  icon?: ReactNode;
  title: string;
  description?: ReactNode;
  action?: ReactNode;
  className?: string;
  compact?: boolean;
};

export function EmptyState({ icon, title, description, action, className, compact }: EmptyStateProps) {
  return (
    <div
      className={cn(
        "flex flex-col items-center justify-center rounded-2xl border border-dashed border-line/90 bg-surface/40 px-6 text-center",
        compact ? "py-8" : "py-16",
        className,
      )}
    >
      {icon ? (
        <div className="mb-3.5 flex h-11 w-11 items-center justify-center rounded-xl border border-line/80 bg-raised text-accent">
          {icon}
        </div>
      ) : null}
      <p className="font-display text-base font-semibold text-ink">{title}</p>
      {description ? (
        <p className="mt-1.5 max-w-md text-sm leading-6 text-muted">{description}</p>
      ) : null}
      {action ? <div className="mt-4">{action}</div> : null}
    </div>
  );
}

export type ErrorStateProps = {
  title?: string;
  message: string;
  requestId?: string;
  onRetry?: () => void;
  className?: string;
  compact?: boolean;
};

export function ErrorState({
  title = "读取失败",
  message,
  requestId,
  onRetry,
  className,
  compact,
}: ErrorStateProps) {
  return (
    <div
      role="alert"
      className={cn(
        "flex flex-col items-center justify-center rounded-2xl border border-danger/25 bg-danger-soft/35 px-6 text-center",
        compact ? "py-8" : "py-14",
        className,
      )}
    >
      <div className="mb-3 flex h-11 w-11 items-center justify-center rounded-xl bg-danger-soft text-danger">
        <AlertCircle className="h-5 w-5" />
      </div>
      <p className="font-display text-base font-semibold text-ink">{title}</p>
      <p className="mt-1.5 max-w-lg text-sm leading-6 text-muted">{message}</p>
      {requestId ? (
        <p className="mt-2 font-mono text-[11px] text-faint">请求编号：{requestId}</p>
      ) : null}
      {onRetry ? (
        <Button className="mt-4" variant="outline" size="sm" leftIcon={<RefreshCw className="h-3.5 w-3.5" />} onClick={onRetry}>
          重试
        </Button>
      ) : null}
    </div>
  );
}
