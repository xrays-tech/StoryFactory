import { create } from "zustand";
import { immer } from "zustand/middleware/immer";

export type ThemeMode = "light" | "dark" | "system";
export type ToastTone = "info" | "success" | "warning" | "error";

export type ToastAction = {
  label: string;
  onClick: () => void;
};

export type ToastItem = {
  id: string;
  title?: string;
  message: string;
  tone: ToastTone;
  action?: ToastAction;
  duration: number;
};

export type ToastInput = {
  title?: string;
  message: string;
  tone?: ToastTone;
  action?: ToastAction;
  duration?: number;
};

export type ConfirmOptions = {
  title: string;
  message: string;
  confirmLabel?: string;
  cancelLabel?: string;
  tone?: "default" | "danger";
};

export type ConfirmState = ConfirmOptions & { id: string };

type UiState = {
  theme: ThemeMode;
  sidebarOpen: boolean;
  toasts: ToastItem[];
  confirmState: ConfirmState | null;
  setTheme: (theme: ThemeMode) => void;
  cycleTheme: () => void;
  setSidebarOpen: (open: boolean) => void;
  toggleSidebar: () => void;
  toast: (input: string | ToastInput) => string;
  dismissToast: (id: string) => void;
  askConfirm: (options: ConfirmOptions) => Promise<boolean>;
  resolveConfirm: (value: boolean) => void;
};

const THEME_KEY = "sa_theme";

function storedTheme(): ThemeMode {
  try {
    const value = localStorage.getItem(THEME_KEY);
    if (value === "light" || value === "dark" || value === "system") return value;
  } catch {
    // ignore
  }
  return "system";
}

export function resolveDark(theme: ThemeMode): boolean {
  if (theme === "dark") return true;
  if (theme === "light") return false;
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}

export function applyTheme(theme: ThemeMode): void {
  const dark = resolveDark(theme);
  document.documentElement.classList.toggle("dark", dark);
  const meta = document.querySelector('meta[name="theme-color"]');
  if (meta) meta.setAttribute("content", dark ? "#181614" : "#f5f0e7");
}

let confirmResolver: ((value: boolean) => void) | null = null;

function makeId(prefix: string): string {
  try {
    return `${prefix}-${crypto.randomUUID()}`;
  } catch {
    return `${prefix}-${Date.now()}-${Math.random().toString(16).slice(2)}`;
  }
}

export const useUiStore = create<UiState>()(
  immer((set, get) => ({
    theme: storedTheme(),
    sidebarOpen: false,
    toasts: [],
    confirmState: null,

    setTheme: (theme) => {
      try {
        localStorage.setItem(THEME_KEY, theme);
      } catch {
        // ignore
      }
      applyTheme(theme);
      set((state) => {
        state.theme = theme;
      });
    },

    cycleTheme: () => {
      const current = get().theme;
      const next: ThemeMode = current === "system" ? "light" : current === "light" ? "dark" : "system";
      get().setTheme(next);
    },

    setSidebarOpen: (open) =>
      set((state) => {
        state.sidebarOpen = open;
      }),

    toggleSidebar: () =>
      set((state) => {
        state.sidebarOpen = !state.sidebarOpen;
      }),

    toast: (input) => {
      const normalized: ToastInput = typeof input === "string" ? { message: input } : input;
      const id = makeId("toast");
      const item: ToastItem = {
        id,
        title: normalized.title,
        message: normalized.message,
        tone: normalized.tone ?? "info",
        action: normalized.action,
        duration: normalized.duration ?? 4200,
      };
      set((state) => {
        state.toasts.push(item);
        if (state.toasts.length > 4) state.toasts.splice(0, state.toasts.length - 4);
      });
      if (item.duration > 0) {
        window.setTimeout(() => get().dismissToast(id), item.duration);
      }
      return id;
    },

    dismissToast: (id) =>
      set((state) => {
        state.toasts = state.toasts.filter((t) => t.id !== id);
      }),

    askConfirm: (options) => {
      confirmResolver?.(false);
      const id = makeId("confirm");
      set((state) => {
        state.confirmState = { ...options, id };
      });
      return new Promise<boolean>((resolve) => {
        confirmResolver = resolve;
      });
    },

    resolveConfirm: (value) => {
      set((state) => {
        state.confirmState = null;
      });
      confirmResolver?.(value);
      confirmResolver = null;
    },
  })),
);
