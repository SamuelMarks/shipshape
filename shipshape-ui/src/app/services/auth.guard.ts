import { CanMatchFn, Router } from "@angular/router";
import { inject } from "@angular/core";

import { AuthService } from "./auth.service";

export const authGuard: CanMatchFn = async () => {
  const auth = inject(AuthService);
  const router = inject(Router);
  const ok = await auth.ensureSession();
  return ok ? true : router.parseUrl("/login");
};
