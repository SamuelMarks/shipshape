/**
 * @file Application bootstrap entrypoint for the ShipShape Angular UI.
 */

import {
  provideHttpClient,
  withInterceptors,
  withXhr,
} from "@angular/common/http";
import { bootstrapApplication } from "@angular/platform-browser";
import { provideRouter } from "@angular/router";

import { AppComponent } from "./app/app.component";
import { appRoutes } from "./app/app.routes";
import { authInterceptor } from "./app/services/auth.interceptor";

/**
 * Root Angular dependency injection providers for routing and HTTP interceptors.
 */
export const appProviders = [
  provideRouter(appRoutes),
  provideHttpClient(withXhr(), withInterceptors([authInterceptor])),
];

/**
 * Bootstrap the root {@link AppComponent} using the provided `bootstrap` function.
 *
 * @param bootstrap - Angular `bootstrapApplication` function.
 * @returns Promise resolving to the bootstrapped `ApplicationRef`.
 */
export function bootstrapApp(bootstrap: typeof bootstrapApplication) {
  return bootstrap(AppComponent, {
    providers: appProviders,
  });
}

/**
 * Check whether the browser environment is outside a Karma/Jasmine test runner.
 *
 * @param env - Global window/environment object to inspect.
 * @returns `true` when neither `__karma__` nor `jasmine` is defined.
 */
export function shouldBootstrap(env: {
  __karma__?: unknown;
  jasmine?: unknown;
}): boolean {
  return (
    typeof env.__karma__ === "undefined" && typeof env.jasmine === "undefined"
  );
}

/**
 * Conditionally bootstrap the Angular application when not running inside unit tests.
 *
 * @param env - Global environment object.
 * @param bootstrap - Angular bootstrap function.
 * @param onError - Error handler invoked if bootstrapping rejects.
 * @returns Bootstrap promise, or `null` when inside a test runner.
 */
export function bootstrapIfReady(
  env: { __karma__?: unknown; jasmine?: unknown } = globalThis as {
    __karma__?: unknown;
    jasmine?: unknown;
  },
  bootstrap: typeof bootstrapApplication = bootstrapApplication,
  onError: (err: unknown) => void = console.error,
) {
  if (!shouldBootstrap(env)) {
    return null;
  }
  return bootstrapApp(bootstrap).catch(onError);
}

bootstrapIfReady();
