import { isRouteErrorResponse, useNavigate, useRouteError } from "react-router-dom";
import { AlertTriangle, ArrowLeft, RefreshCw } from "lucide-react";
import { userMessage } from "../lib/errors";
import { Button } from "../components/common/Button";
import { zh } from "../copy/zh";

export function RouteErrorPage() {
  const error = useRouteError();
  const navigate = useNavigate();
  const message = isRouteErrorResponse(error)
    ? `${error.status} ${error.statusText || ""}`
    : userMessage(error);
  const requestId = (error as { requestId?: string })?.requestId;

  return (
    <div className="flex min-h-dvh items-center justify-center px-6">
      <div className="w-full max-w-xl text-center">
        <div className="mx-auto mb-4 flex h-14 w-14 items-center justify-center rounded-2xl bg-danger-soft text-danger">
          <AlertTriangle className="h-7 w-7" />
        </div>
        <h1 className="font-display text-2xl font-semibold text-ink">页面加载失败</h1>
        <p className="mt-2 text-sm leading-6 text-muted">{message}</p>
        {requestId ? <p className="mt-2 font-mono text-[11px] text-faint">{zh.errors.requestId}：{requestId}</p> : null}
        <div className="mt-5 flex flex-wrap justify-center gap-2">
          <Button
            variant="primary"
            leftIcon={<RefreshCw className="h-4 w-4" />}
            onClick={() => window.location.reload()}
          >
            重新加载
          </Button>
          <Button variant="outline" leftIcon={<ArrowLeft className="h-4 w-4" />} onClick={() => navigate("/")}>
            返回项目
          </Button>
        </div>
      </div>
    </div>
  );
}
