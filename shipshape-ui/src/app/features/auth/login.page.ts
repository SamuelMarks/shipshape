import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { NgOptimizedImage } from '@angular/common';

import { AuthService, BROWSER_WINDOW } from '../../services/auth.service';

@Component({
  selector: 'shipshape-login-page',
  templateUrl: './login.page.html',
  styleUrl: './login.page.css',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [NgOptimizedImage]
})
export class LoginPage {
  private readonly auth = inject(AuthService);
  private readonly windowRef = inject(BROWSER_WINDOW);

  async startLogin(): Promise<void> {
    const url = await this.auth.getLoginUrl();
    this.windowRef.location.assign(url);
  }
}
