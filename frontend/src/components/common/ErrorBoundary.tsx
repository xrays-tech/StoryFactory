import { Component, type ErrorInfo, type ReactNode } from "react";
import { AlertTriangle, ArrowLeft, RotateCcw } from "lucide-react";
import { reportClientError } from "../../api/client";
import { zh } from "../../copy/zh";
import { Button } from "./Button";

type Props = { children: ReactNode };
type State = { error: Error | null };

export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    void reportClientError({
      message: error.message,
      stack: error.stack,
      component_stack: info.componentStack ?? undefined,
      route: window.location.pathname,
      user_agent: navigator.userAgent,
      at: Date.now(),
    });
  }

  render(): ReactNode {
    const { error } = this.state;
    if (!error) return this.props.children;

    return (
      <div className="mx-auto flex min-h-[60vh] max-w-2xl flex-col items-center justify-center px-6 text-center">
        <div className="mb-4 flex h-14 w-14 items-center justify-center rounded-2xl bg-danger-soft text-danger">
          <AlertTriangle className="h-7 w-7" />
        </div>
        <h1 className="font-display text-2xl font-semibold text-ink">{zh.common.error}</h1>
        <p className="mt-2 text-sm leading-6 text-muted">
          页面渲染时出现问题。已经尝试把错误上报到本地日志，你可以重试或返回项目列表。
        </p>
        <p className="mt-3 max-w-xl break-words rounded-lg border border-line/70 bg-surface px-3 py-2 font-mono text-xs text-faint">
          {error.message}
        </p>
        <div className="mt-5 flex flex-wrap justify-center gap-2">
          <Button
            variant="primary"
            leftIcon={<RotateCcw className="h-4 w-4" />}
            onClick={() => window.location.reload()}
          >
            重新加载
          </Button>
          <Button
            variant="outline"
            leftIcon={<ArrowLeft className="h-4 w-4" />}
            onClick={() => {
              window.location.href = "/";
            }}
          >
            返回首页
          </Button>
        </div>
      </div>
    );
  }
}
