/**
 * @file HTTP interceptor attaching the ShipShape bearer token to authenticated
 * API requests while skipping public OAuth configuration/exchange endpoints.
 */

import { HttpInterceptorFn } from "@angular/common/http";
import { inject } from "@angular/core";

import { AuthService } from "./auth.service";

/** Public authentication endpoint suffixes that do not require a bearer token. */
const PUBLIC_AUTH_SUFFIXES = [
  "/auth/config",
  "/auth/github",
  "/auth/github/token",
];

/**
 * Determine whether `url` targets a public OAuth endpoint by inspecting its path
 * (excluding query parameters).
 *
 * @param url - Request URL string.
 * @returns `true` if `url` ends with one of {@link PUBLIC_AUTH_SUFFIXES}.
 */
export function isPublicAuthUrl(url: string): boolean {
  const pathPart = url.split("?")[0];
  return PUBLIC_AUTH_SUFFIXES.some((suffix) => pathPart.endsWith(suffix));
}

/**
 * Angular HTTP interceptor that attaches `Authorization: Bearer <token>` to
 * outgoing API requests when a session token is present.
 *
 * @param req - Outgoing HTTP request.
 * @param next - Next interceptor handler in the chain.
 * @returns Observable of the HTTP event stream.
 */
export const authInterceptor: HttpInterceptorFn = (req, next) => {
  const auth = inject(AuthService);
  const token = auth.token();
  if (!token || isPublicAuthUrl(req.url)) {
    return next(req);
  }
  return next(
    req.clone({
      setHeaders: {
        Authorization: `Bearer ${token}`,
      },
    }),
  );
};
