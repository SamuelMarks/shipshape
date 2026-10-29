/**
 * @file Route guard protecting authenticated pages and redirecting
 * unauthenticated visitors to `/login`.
 */

import { CanMatchFn, Router } from "@angular/router";
import { inject } from "@angular/core";

import { AuthService } from "./auth.service";

/**
 * Angular `CanMatchFn` guard verifying an active ShipShape session via
 * {@link AuthService.ensureSession} or redirecting to `/login`.
 *
 * @returns Promise resolving to `true` when authenticated or a `/login` `UrlTree`.
 */
export const authGuard: CanMatchFn = async () => {
  const auth = inject(AuthService);
  const router = inject(Router);
  const ok = await auth.ensureSession();
  return ok ? true : router.parseUrl("/login");
};
