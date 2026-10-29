/**
 * @file Root application shell component providing primary navigation,
 * skip-link focus management, and session sign-out controls.
 */

import { Component, DestroyRef, inject } from "@angular/core";
import { DOCUMENT, NgOptimizedImage } from "@angular/common";
import {
  NavigationEnd,
  Router,
  RouterLink,
  RouterLinkActive,
  RouterOutlet,
} from "@angular/router";
import { filter } from "rxjs/operators";
import { takeUntilDestroyed } from "@angular/core/rxjs-interop";

import { AuthService } from "./services/auth.service";

/**
 * Root shell component hosting navigation links and the primary `<router-outlet>`.
 */
@Component({
  selector: "shipshape-root",
  templateUrl: "./app.component.html",
  styleUrl: "./app.component.css",
  imports: [RouterOutlet, RouterLink, RouterLinkActive, NgOptimizedImage],
  host: {
    class: "app-shell",
  },
})
export class AppComponent {
  /** Angular router service. */
  private readonly router = inject(Router);
  /** Injected DOM document reference. */
  private readonly document = inject(DOCUMENT);
  /** Component destroy reference for cleaning up router event subscriptions. */
  private readonly destroyRef = inject(DestroyRef);
  /** Authentication state service. */
  private readonly auth = inject(AuthService);

  /** Current authenticated user signal. */
  readonly user = this.auth.user;
  /** Signal indicating whether the user is currently authenticated. */
  readonly isAuthenticated = this.auth.isAuthenticated;

  /** Primary sidebar navigation items. */
  readonly navItems = [
    {
      path: "/dashboard",
      label: "Dashboard",
      hint: "Fleet health overview",
    },
    {
      path: "/batch",
      label: "Batch View",
      hint: "Parallel audit runs",
    },
    {
      path: "/workflow",
      label: "Workflow Studio",
      hint: "Launch new projects",
    },
    {
      path: "/diff",
      label: "Diff Viewer",
      hint: "Review refit changes",
    },
    {
      path: "/control",
      label: "Control Room",
      hint: "Launch workflows",
    },
  ] as const;

  /**
   * Subscribe to router navigation completion events to move focus to `#main-content`.
   */
  constructor() {
    this.router.events
      .pipe(
        filter((event) => event instanceof NavigationEnd),
        takeUntilDestroyed(this.destroyRef),
      )
      .subscribe(() => this.focusMain());
  }

  /**
   * Move keyboard focus to the `#main-content` landmark element if present.
   */
  private focusMain(): void {
    const main = this.document.getElementById("main-content");
    if (main) {
      main.focus();
    }
  }

  /**
   * Clear the current authentication session and redirect to `/login`.
   */
  signOut(): void {
    this.auth.signOut();
    void this.router.navigate(["/login"]);
  }
}
