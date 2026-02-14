import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  inject,
} from "@angular/core";
import { NgOptimizedImage } from "@angular/common";
import {
  NavigationEnd,
  Router,
  RouterLink,
  RouterLinkActive,
  RouterOutlet,
} from "@angular/router";
import { filter } from "rxjs/operators";
import { takeUntilDestroyed } from "@angular/core/rxjs-interop";
import { DOCUMENT } from "@angular/common";

import { AuthService } from "./services/auth.service";

@Component({
  selector: "shipshape-root",
  templateUrl: "./app.component.html",
  styleUrl: "./app.component.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [RouterOutlet, RouterLink, RouterLinkActive, NgOptimizedImage],
  host: {
    class: "app-shell",
  },
})
export class AppComponent {
  private readonly router = inject(Router);
  private readonly document = inject(DOCUMENT);
  private readonly destroyRef = inject(DestroyRef);
  private readonly auth = inject(AuthService);

  readonly user = this.auth.user;
  readonly isAuthenticated = this.auth.isAuthenticated;

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

  constructor() {
    this.router.events
      .pipe(
        filter((event) => event instanceof NavigationEnd),
        takeUntilDestroyed(this.destroyRef),
      )
      .subscribe(() => this.focusMain());
  }

  private focusMain(): void {
    const main = this.document.getElementById("main-content");
    if (main) {
      main.focus();
    }
  }

  signOut(): void {
    this.auth.signOut();
    void this.router.navigate(["/login"]);
  }
}
