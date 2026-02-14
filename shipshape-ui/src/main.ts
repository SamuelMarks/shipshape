import { provideHttpClient, withInterceptors } from "@angular/common/http";
import { bootstrapApplication } from "@angular/platform-browser";
import { provideAnimations } from "@angular/platform-browser/animations";
import { provideRouter } from "@angular/router";

import { AppComponent } from "./app/app.component";
import { appRoutes } from "./app/app.routes";
import { authInterceptor } from "./app/services/auth.interceptor";

export const appProviders = [
  provideRouter(appRoutes),
  provideAnimations(),
  provideHttpClient(withInterceptors([authInterceptor])),
];

export function bootstrapApp(bootstrap: typeof bootstrapApplication) {
  return bootstrap(AppComponent, {
    providers: appProviders,
  });
}

export function shouldBootstrap(env: {
  __karma__?: unknown;
  jasmine?: unknown;
}): boolean {
  return (
    typeof env.__karma__ === "undefined" && typeof env.jasmine === "undefined"
  );
}

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
