import { NavLink } from "react-router-dom";
import { Feather, LibraryBig, Settings, Trash2, X } from "lucide-react";
import { cn } from "../../lib/cn";
import { zh } from "../../copy/zh";
import { useUiStore } from "../../stores/uiStore";

const navItems = [
  { to: "/", label: zh.nav.dashboard, icon: LibraryBig, end: true },
  { to: "/recycle", label: zh.nav.recycle, icon: Trash2, end: false },
  { to: "/settings", label: zh.nav.settings, icon: Settings, end: false },
];

export function Sidebar({ collapsed = false }: { collapsed?: boolean }) {
  return (
    <div className="flex h-full flex-col">
      <div className={cn("flex items-center gap-3 px-4 pb-6 pt-5", collapsed && "justify-center px-2")}>
        <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-xl bg-accent text-white shadow-sm dark:text-[#201612]">
          <Feather className="h-[18px] w-[18px]" />
        </div>
        {!collapsed ? (
          <div className="min-w-0">
            <p className="font-display text-base font-semibold leading-tight tracking-tight text-ink">
              {zh.appName}
            </p>
            <p className="truncate text-[11px] tracking-wide text-faint">{zh.appSubtitle}</p>
          </div>
        ) : null}
      </div>

      <nav className="flex-1 space-y-1 px-2.5" aria-label="主导航">
        {navItems.map((item) => {
          const Icon = item.icon;
          return (
            <NavLink
              key={item.to}
              to={item.to}
              end={item.end}
              title={collapsed ? item.label : undefined}
              className={({ isActive }) =>
                cn(
                  "focus-ring group relative flex items-center gap-3 rounded-lg px-3 py-2.5 text-sm transition-colors duration-150",
                  collapsed && "justify-center px-0",
                  isActive
                    ? "bg-accent-soft/70 font-medium text-accent-strong dark:text-accent"
                    : "text-muted hover:bg-line/30 hover:text-ink",
                )
              }
            >
              {({ isActive }) => (
                <>
                  {isActive ? (
                    <span className="absolute left-0 top-1/2 h-5 w-0.5 -translate-y-1/2 rounded-full bg-accent" />
                  ) : null}
                  <Icon className="h-[18px] w-[18px] shrink-0" />
                  {!collapsed ? <span>{item.label}</span> : null}
                </>
              )}
            </NavLink>
          );
        })}
      </nav>

      {!collapsed ? (
        <div className="m-3 rounded-xl border border-line/70 bg-paper/60 px-3 py-2.5">
          <p className="flex items-center gap-1.5 text-[11px] font-medium text-muted">
            <span className="h-1.5 w-1.5 rounded-full bg-success" />
            本地优先 · 数据留在本机
          </p>
          <p className="mt-1 text-[10px] leading-4 text-faint">StoryArchitect AI v4.0</p>
        </div>
      ) : null}
    </div>
  );
}

export function MobileSidebar() {
  const open = useUiStore((s) => s.sidebarOpen);
  const setOpen = useUiStore((s) => s.setSidebarOpen);

  if (!open) return null;

  return (
    <div className="fixed inset-0 z-[60] md:hidden">
      <div
        className="absolute inset-0 bg-ink/35 backdrop-blur-[2px] animate-fade-in dark:bg-black/60"
        onClick={() => setOpen(false)}
        aria-hidden
      />
      <aside
        className="relative h-full w-[min(84vw,300px)] border-r border-line/80 bg-surface shadow-float animate-slide-in"
        aria-label="导航"
      >
        <button
          type="button"
          onClick={() => setOpen(false)}
          className="focus-ring absolute right-3 top-4 rounded-lg p-1.5 text-faint hover:bg-line/40 hover:text-ink"
          aria-label="关闭导航"
        >
          <X className="h-4 w-4" />
        </button>
        <Sidebar />
      </aside>
    </div>
  );
}
