import { Route } from '@angular/router';

import { appRoutes } from './app.routes';

describe('appRoutes', () => {
  it('defines the main feature routes', () => {
    const paths = appRoutes.map((route) => route.path);
    expect(paths).toContain('login');
    expect(paths).toContain('auth/callback');
    expect(paths).toContain('dashboard');
    expect(paths).toContain('batch');
    expect(paths).toContain('diff');
    expect(paths).toContain('control');
  });

  it('redirects the empty path to dashboard', () => {
    const route = appRoutes.find((candidate) => candidate.path === '');
    expect(route?.redirectTo).toBe('dashboard');
  });

  it('lazy loads feature components', async () => {
    const load = async (path: string) => {
      const route = appRoutes.find((candidate) => candidate.path === path) as Route;
      expect(route?.loadComponent).toBeDefined();
      await route.loadComponent?.();
    };

    await load('dashboard');
    await load('batch');
    await load('diff');
    await load('control');
    await load('login');
    await load('auth/callback');
  });

  it('protects primary routes with auth guard', () => {
    const route = appRoutes.find((candidate) => candidate.path === 'dashboard');
    expect(route?.canMatch?.length).toBe(1);
  });
});
