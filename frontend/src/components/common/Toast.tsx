import { AlertTriangle, CheckCircle2, Info, X, XCircle } from "lucide-react";
import type { ToastTone } from "../../stores/uiStore";
import { useUiStore } from "../../stores/uiStore";
import { cn } from "../../lib/cn";

const icons = {
  info: Info,
  success: CheckCircle2,
  warning: AlertTriangle,
  error: XCircle,
} as const;

const tones: Record<ToastTone, string> = {
  info: "border-info/25 bg-info-soft/95 text-info",
  success: "border-success/25 bg-success-soft/95 text-success",
  warning: "border-warning/30 bg-warning-soft/95 text-warning",
  error: "border-danger/25 bg-danger-soft/95 text-danger",
};

export function ToastViewport() {
  const toasts = useUiStore((s) => s.toasts);
  const dismiss = useUiStore((s) => s.dismissToast);

  return (
    <div
      className="pointer-events-none fixed bottom-4 right-4 z-[90] flex w-[min(92vw,380px)] flex-col gap-2"
      aria-live="polite"
      aria-atomic="false"
    >
      {toasts.map((toast) => {
        const Icon = icons[toast.tone];
        return (
          <div
            key={toast.id}
            role="status"
            className={cn(
              "pointer-events-auto flex items-start gap-3 rounded-xl border px-3.5 py-3 shadow-float backdrop-blur-md animate-slide-in",
              tones[toast.tone],
            )}
          >
            <Icon className="mt-0.5 h-4 w-4 shrink-0" aria-hidden />
            <div className="min-w-0 flex-1">
              {toast.title ? <p className="text-sm font-semibold">{toast.title}</p> : null}
              <p className={cn("text-sm leading-6", toast.title && "mt-0.5 opacity-90")}>
                {toast.message}
              </p>
              {toast.action ? (
                <button
                  type="button"
                  onClick={() => {
                    toast.action?.onClick();
                    dismiss(toast.id);
                  }}
                  className="focus-ring mt-1.5 rounded text-xs font-semibold underline underline-offset-2 opacity-90 hover:opacity-100"
                >
                  {toast.action.label}
                </button>
              ) : null}
            </div>
            <button
              type="button"
              onClick={() => dismiss(toast.id)}
              className="focus-ring rounded p-0.5 opacity-60 transition-opacity hover:opacity-100"
              aria-label="关闭提示"
            >
              <X className="h-3.5 w-3.5" />
            </button>
          </div>
        );
      })}
    </div>
  );
}
