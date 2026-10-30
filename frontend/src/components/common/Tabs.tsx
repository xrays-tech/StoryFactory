import { NavLink } from "react-router-dom";
import type { ReactNode } from "react";
import { cn } from "../../lib/cn";

export type NavTabItem = {
  to: string;
  label: string;
  icon?: ReactNode;
  end?: boolean;
  hint?: string;
};

export function NavTabs({ items, className }: { items: NavTabItem[]; className?: string }) {
  return (
    <nav
      className={cn(
        "no-scrollbar flex items-center gap-1 overflow-x-auto border-b border-line/70",
        className,
      )}
      aria-label="项目分区"
    >
      {items.map((item) => (
        <NavLink
          key={item.to}
          to={item.to}
          end={item.end}
          title={item.hint}
          className={({ isActive }) =>
            cn(
              "focus-ring relative flex shrink-0 items-center gap-2 rounded-t-lg px-3.5 py-2.5 text-sm transition-colors duration-150",
              isActive
                ? "font-medium text-ink"
                : "text-muted hover:bg-line/25 hover:text-ink",
            )
          }
        >
          {({ isActive }) => (
            <>
              {item.icon}
              <span>{item.label}</span>
              {isActive ? (
                <span className="absolute inset-x-2 -bottom-px h-0.5 rounded-full bg-accent" />
              ) : null}
            </>
          )}
        </NavLink>
      ))}
    </nav>
  );
}

export type TabItem<T extends string> = {
  value: T;
  label: ReactNode;
  badge?: ReactNode;
};

export function Tabs<T extends string>({
  value,
  onChange,
  items,
  className,
}: {
  value: T;
  onChange: (value: T) => void;
  items: Array<TabItem<T>>;
  className?: string;
}) {
  return (
    <div
      role="tablist"
      className={cn(
        "no-scrollbar inline-flex max-w-full items-center gap-0.5 overflow-x-auto rounded-lg border border-line/80 bg-paper/60 p-0.5",
        className,
      )}
    >
      {items.map((item) => {
        const active = item.value === value;
        return (
          <button
            key={item.value}
            role="tab"
            type="button"
            aria-selected={active}
            onClick={() => onChange(item.value)}
            className={cn(
              "focus-ring inline-flex shrink-0 items-center gap-1.5 whitespace-nowrap rounded-md px-2.5 py-1.5 text-xs font-medium transition-colors duration-150",
              active ? "bg-raised text-ink shadow-sm" : "text-muted hover:text-ink",
            )}
          >
            {item.label}
            {item.badge}
          </button>
        );
      })}
    </div>
  );
}
