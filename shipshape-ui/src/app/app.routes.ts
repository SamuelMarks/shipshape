import { Routes } from '@angular/router';

import { authGuard } from './services/auth.guard';

export const appRoutes: Routes = [
  {
    path: '',
    pathMatch: 'full',
    redirectTo: 'dashboard'
  },
  {
    path: 'login',
    loadComponent: () =>
      import('./features/auth/login.page').then((m) => m.LoginPage)
  },
  {
    path: 'auth/callback',
    loadComponent: () =>
      import('./features/auth/auth-callback.page').then(
        (m) => m.AuthCallbackPage
      )
  },
  {
    path: 'dashboard',
    canMatch: [authGuard],
    loadComponent: () =>
      import('./features/dashboard/dashboard.page').then((m) => m.DashboardPage)
  },
  {
    path: 'batch',
    canMatch: [authGuard],
    loadComponent: () =>
      import('./features/batch/batch.page').then((m) => m.BatchPage)
  },
  {
    path: 'diff',
    canMatch: [authGuard],
    loadComponent: () =>
      import('./features/diff/diff.page').then((m) => m.DiffPage)
  },
  {
    path: 'control',
    canMatch: [authGuard],
    loadComponent: () =>
      import('./features/control/control.page').then((m) => m.ControlRoomPage)
  },
  {
    path: '**',
    redirectTo: 'dashboard'
  }
];
