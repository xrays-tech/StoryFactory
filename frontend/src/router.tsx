import { createBrowserRouter, type LoaderFunctionArgs } from "react-router-dom";
import { AppLayout } from "./components/layout/AppLayout";
import { Dashboard } from "./pages/Dashboard";
import { NovelDetail } from "./pages/NovelDetail";
import { NovelOverview, overviewLoader } from "./pages/novel/Overview";
import { CharactersPage, charactersLoader } from "./pages/novel/Characters";
import { RelationsPage, relationsLoader } from "./pages/novel/Relations";
import { OutlinePage, outlineLoader } from "./pages/novel/Outline";
import { ItemsPage, itemsLoader } from "./pages/novel/Items";
import { SandboxPage, sandboxListLoader } from "./pages/SandboxPage";
import { SandboxConsole, sandboxConsoleLoader } from "./pages/SandboxConsole";
import { SettingsPage, settingsLoader } from "./pages/Settings";
import { RecyclePage } from "./pages/RecyclePage";
import { NotFoundPage } from "./pages/NotFound";
import { RouteErrorPage } from "./pages/RouteError";
import { useNovelStore } from "./stores/novelStore";

async function dashboardLoader() {
  await useNovelStore.getState().load({ page: 1 });
  return null;
}

async function novelLoader({ params }: LoaderFunctionArgs) {
  if (params.novelId) await useNovelStore.getState().loadOne(params.novelId);
  return null;
}

export const router = createBrowserRouter([
  {
    path: "/",
    element: <AppLayout />,
    errorElement: <RouteErrorPage />,
    children: [
      { index: true, element: <Dashboard />, loader: dashboardLoader },
      {
        path: "novel/:novelId",
        element: <NovelDetail />,
        loader: novelLoader,
        children: [
          { index: true, element: <NovelOverview />, loader: overviewLoader },
          { path: "characters", element: <CharactersPage />, loader: charactersLoader },
          { path: "relations", element: <RelationsPage />, loader: relationsLoader },
          { path: "outline", element: <OutlinePage />, loader: outlineLoader },
          { path: "items", element: <ItemsPage />, loader: itemsLoader },
          { path: "sandbox", element: <SandboxPage />, loader: sandboxListLoader },
        ],
      },
      {
        path: "sandbox/:sid",
        element: <SandboxConsole />,
        loader: sandboxConsoleLoader,
      },
      { path: "settings", element: <SettingsPage />, loader: settingsLoader },
      { path: "recycle", element: <RecyclePage /> },
      { path: "*", element: <NotFoundPage /> },
    ],
  },
]);
