/**
 * @file Login page component initiating the GitHub OAuth sign-in redirect.
 */

import { Component, inject } from "@angular/core";
import { NgOptimizedImage } from "@angular/common";

import { AuthService, BROWSER_WINDOW } from "../../services/auth.service";

/**
 * Page component presenting the ShipShape sign-in prompt and redirecting to GitHub OAuth.
 */
@Component({
  selector: "shipshape-login-page",
  templateUrl: "./login.page.html",
  styleUrl: "./login.page.css",
  imports: [NgOptimizedImage],
})
export class LoginPage {
  /** Authentication state and URL builder service. */
  private readonly auth = inject(AuthService);
  /** Browser window reference used for location navigation. */
  private readonly windowRef = inject(BROWSER_WINDOW);

  /**
   * Fetch the GitHub OAuth login URL and navigate the browser window to it.
   *
   * @returns Promise resolving once navigation is triggered.
   */
  async startLogin(): Promise<void> {
    const url = await this.auth.getLoginUrl();
    this.windowRef.location.assign(url);
  }
}
