/**
 * @file OAuth callback page component exchanging the GitHub authorization code
 * for a ShipShape session token.
 */

import { Component, DestroyRef, inject, signal } from "@angular/core";
import { ActivatedRoute, Router, RouterLink } from "@angular/router";
import { takeUntilDestroyed } from "@angular/core/rxjs-interop";

import { AuthService } from "../../services/auth.service";

/**
 * Page component handling the `/auth/callback` route and completing sign-in.
 */
@Component({
  selector: "shipshape-auth-callback-page",
  templateUrl: "./auth-callback.page.html",
  styleUrl: "./auth-callback.page.css",
  imports: [RouterLink],
})
export class AuthCallbackPage {
  /** Active route snapshot/observable provider. */
  private readonly route = inject(ActivatedRoute);
  /** Angular router for redirecting to `/dashboard` after login. */
  private readonly router = inject(Router);
  /** Authentication state and code exchange service. */
  private readonly auth = inject(AuthService);
  /** Component destroy reference for cleaning up query parameter subscriptions. */
  private readonly destroyRef = inject(DestroyRef);

  /** Current callback exchange status (`"loading"`, `"error"`, or `"success"`). */
  readonly status = signal<"loading" | "error" | "success">("loading");
  /** User-facing status message. */
  readonly message = signal("Completing sign-in...");

  /**
   * Subscribe to route query parameters and initiate the OAuth code exchange.
   */
  constructor() {
    this.route.queryParamMap
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe((params) => {
        const code = params.get("code");
        if (!code) {
          this.status.set("error");
          this.message.set("Missing authorization code.");
          return;
        }
        void this.completeAuth(code);
      });
  }

  /**
   * Exchange `code` with the backend and navigate to `/dashboard` on success.
   *
   * @param code - GitHub OAuth authorization code.
   */
  private async completeAuth(code: string): Promise<void> {
    try {
      await this.auth.exchangeCode(code);
      this.status.set("success");
      this.message.set("Signed in. Redirecting to your dashboard...");
      await this.router.navigate(["/dashboard"]);
    } catch {
      this.status.set("error");
      this.message.set("Sign-in failed. Please retry.");
    }
  }
}
