import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { RouterProvider } from "react-router-dom";
import { router } from "./router";
import { bootstrapAppToken } from "./api/client";
import { applyTheme, useUiStore } from "./stores/uiStore";
import "./styles.css";

bootstrapAppToken();
applyTheme(useUiStore.getState().theme);

const media = window.matchMedia("(prefers-color-scheme: dark)");
const onSystemThemeChange = () => {
  if (useUiStore.getState().theme === "system") applyTheme("system");
};
media.addEventListener?.("change", onSystemThemeChange);

function BootScreen() {
  return (
    <div className="flex min-h-dvh items-center justify-center bg-paper text-ink">
      <div className="flex flex-col items-center gap-3">
        <div className="flex h-12 w-12 items-center justify-center rounded-2xl bg-accent text-white shadow-float dark:text-[#201612]">
          <span className="font-display text-xl font-semibold">故</span>
        </div>
        <p className="text-sm text-muted">正在打开故事建筑师…</p>
      </div>
    </div>
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <RouterProvider router={router} fallbackElement={<BootScreen />} />
  </StrictMode>,
);
