import { useEffect } from "react";
import { Outlet, useLocation, useNavigation } from "react-router-dom";
import { Menu, Monitor, Moon, Sun } from "lucide-react";
import { useUiStore, type ThemeMode } from "../../stores/uiStore";
import { MobileSidebar, Sidebar } from "./Sidebar";
import { ErrorBoundary } from "../common/ErrorBoundary";
import { ConfirmDialog } from "../common/ConfirmDialog";
import { ToastViewport } from "../common/Toast";
import { zh } from "../../copy/zh";

function pageTitle(pathname: string): string {
  if (pathname.startsWith("/novels/new")) return zh.nav.importNovel;
  if (pathname.startsWith("/settings")) return zh.nav.settings;
  if (pathname.startsWith("/recycle")) return zh.nav.recycle;
  if (pathname.startsWith("/sandbox")) return "沙盒控制台";
  if (pathname.startsWith("/novel")) return "项目工作台";
  return zh.nav.dashboard;
}

function ThemeIcon({ theme }: { theme: ThemeMode }) {
  if (theme === "light") return <Sun className="h-4 w-4" />;
  if (theme === "dark") return <Moon className="h-4 w-4" />;
  return <Monitor className="h-4 w-4" />;
}

const themeLabel: Record<ThemeMode, string> = {
  light: "浅色",
  dark: "深色",
  system: "跟随系统",
};

export function AppLayout() {
  const location = useLocation();
  const navigation = useNavigation();
  const theme = useUiStore((s) => s.theme);
  const cycleTheme = useUiStore((s) => s.cycleTheme);
  const toggleSidebar = useUiStore((s) => s.toggleSidebar);
  const setSidebarOpen = useUiStore((s) => s.setSidebarOpen);

  useEffect(() => {
    setSidebarOpen(false);
  }, [location.pathname, setSidebarOpen]);

  useEffect(() => {
    const onApiError = (event: Event) => {
      const error = (event as CustomEvent).detail as
        | { status?: number; message?: string; requestId?: string }
        | undefined;
      if (!error?.status) return;
      const toast = useUiStore.getState().toast;
      if (error.status === 422) {
        toast({
          tone: "warning",
          title: "需要修改",
          message: `${error.message ?? "提交内容未通过校验"} 请检查标红字段。`,
        });
      } else if (error.status === 409) {
        toast({
          tone: "error",
          title: "状态冲突",
          message: `${error.message ?? "当前状态不允许这个操作"} 已为你保留当前页面，可刷新后重试。`,
        });
      } else if (error.status === 429) {
        toast({
          tone: "error",
          title: "请求受限",
          message: error.message ?? "并发或预算已达上限，请稍后重试。",
        });
      }
    };
    window.addEventListener("sa:api-error", onApiError);
    return () => window.removeEventListener("sa:api-error", onApiError);
  }, []);

  return (
    <div className="relative z-10 mx-auto flex min-h-dvh w-full max-w-[1920px]">
      <a
        href="#main-content"
        className="sr-only z-[100] rounded-lg bg-raised px-3 py-2 text-sm shadow-float focus:not-sr-only focus:fixed focus:left-4 focus:top-4"
      >
        跳到主要内容
      </a>

      <aside className="sticky top-0 hidden h-dvh w-[76px] shrink-0 border-r border-line/70 bg-surface/65 backdrop-blur-sm md:flex xl:w-64">
        <div className="hidden w-full xl:block">
          <Sidebar />
        </div>
        <div className="w-full xl:hidden">
          <Sidebar collapsed />
        </div>
      </aside>

      <MobileSidebar />

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="sticky top-0 z-40 flex h-14 items-center justify-between gap-3 border-b border-line/70 bg-paper/80 px-4 backdrop-blur-md sm:px-6 lg:px-8">
          <div className="flex min-w-0 items-center gap-2.5">
            <button
              type="button"
              onClick={toggleSidebar}
              className="focus-ring rounded-lg p-1.5 text-muted hover:bg-line/40 hover:text-ink md:hidden"
              aria-label="打开导航"
            >
              <Menu className="h-5 w-5" />
            </button>
            <div className="flex min-w-0 items-center gap-2 text-sm">
              <span className="hidden text-faint sm:inline">{zh.appName}</span>
              <span className="hidden text-faint/60 sm:inline">/</span>
              <span className="truncate font-medium text-ink">{pageTitle(location.pathname)}</span>
            </div>
          </div>

          <div className="flex items-center gap-1.5">
            <button
              type="button"
              onClick={cycleTheme}
              title={`主题：${themeLabel[theme]}（点击切换）`}
              className="focus-ring inline-flex h-8 items-center gap-1.5 rounded-lg border border-line/70 bg-raised/70 px-2 text-xs text-muted transition-colors hover:text-ink"
              aria-label={`当前主题：${themeLabel[theme]}，点击切换`}
            >
              <ThemeIcon theme={theme} />
              <span className="hidden sm:inline">{themeLabel[theme]}</span>
            </button>
          </div>

          {navigation.state === "loading" ? (
            <div className="absolute inset-x-0 bottom-0 h-0.5 overflow-hidden bg-accent/10">
              <div className="h-full w-1/3 animate-indeterminate bg-accent/70" />
            </div>
          ) : null}
        </header>

        <main id="main-content" className="min-w-0 flex-1 px-4 py-6 sm:px-6 lg:px-8 lg:py-8">
          <ErrorBoundary key={location.pathname}>
            <Outlet />
          </ErrorBoundary>
        </main>
      </div>

      <ToastViewport />
      <ConfirmDialog />
    </div>
  );
}
