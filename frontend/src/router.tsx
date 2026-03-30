import { QueryClient } from "@tanstack/react-query";
import {
  Outlet,
  RouterProvider,
  createRootRouteWithContext,
  createRoute,
  createRouter,
} from "@tanstack/react-router";

import { AppShell } from "@/shell/app-shell";
import { DeskPage } from "@/routes/desk-page";
import { WorkspacePage } from "@/routes/workspace-page";
import { SignalsPage } from "@/routes/signals-page";

export interface RouterContext {
  queryClient: QueryClient;
}

const rootRoute = createRootRouteWithContext<RouterContext>()({
  component: () => (
    <AppShell>
      <Outlet />
    </AppShell>
  ),
});

const deskRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  component: DeskPage,
});

const workspaceRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/workspace",
  component: WorkspacePage,
});

const signalsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/signals",
  component: SignalsPage,
});

const routeTree = rootRoute.addChildren([deskRoute, workspaceRoute, signalsRoute]);

export const router = createRouter({
  routeTree,
  context: {
    queryClient: undefined as unknown as QueryClient,
  },
});

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
