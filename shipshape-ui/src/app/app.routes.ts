/**
 * @file Application route table definitions for the ShipShape UI.
 */

import { Routes } from "@angular/router";

import { authGuard } from "./services/auth.guard";

/**
 * Top-level Angular route configuration with lazy-loaded feature pages
 * protected by {@link authGuard}.
 */
export const appRoutes: Routes = [
  {
    path: "",
    pathMatch: "full",
    redirectTo: "dashboard",
  },
  {
    path: "login",
    loadComponent: () =>
      import("./features/auth/login.page").then((m) => m.LoginPage),
  },
  {
    path: "auth/callback",
    loadComponent: () =>
      import("./features/auth/auth-callback.page").then(
        (m) => m.AuthCallbackPage,
      ),
  },
  {
    path: "dashboard",
    canMatch: [authGuard],
    loadComponent: () =>
      import("./features/dashboard/dashboard.page").then(
        (m) => m.DashboardPage,
      ),
  },
  {
    path: "batch",
    canMatch: [authGuard],
    loadComponent: () =>
      import("./features/batch/batch.page").then((m) => m.BatchPage),
  },
  {
    path: "workflow",
    canMatch: [authGuard],
    loadComponent: () =>
      import("./features/workflow/workflow.page").then((m) => m.WorkflowPage),
  },
  {
    path: "diff",
    canMatch: [authGuard],
    loadComponent: () =>
      import("./features/diff/diff.page").then((m) => m.DiffPage),
  },
  {
    path: "control",
    canMatch: [authGuard],
    loadComponent: () =>
      import("./features/control/control.page").then((m) => m.ControlRoomPage),
  },
  {
    path: "**",
    redirectTo: "dashboard",
  },
];
