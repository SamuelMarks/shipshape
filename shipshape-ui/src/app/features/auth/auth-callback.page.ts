import { ChangeDetectionStrategy, Component, DestroyRef, inject, signal } from '@angular/core';
import { ActivatedRoute, Router, RouterLink } from '@angular/router';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';

import { AuthService } from '../../services/auth.service';

@Component({
  selector: 'shipshape-auth-callback-page',
  templateUrl: './auth-callback.page.html',
  styleUrl: './auth-callback.page.css',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [RouterLink]
})
export class AuthCallbackPage {
  private readonly route = inject(ActivatedRoute);
  private readonly router = inject(Router);
  private readonly auth = inject(AuthService);
  private readonly destroyRef = inject(DestroyRef);

  readonly status = signal<'loading' | 'error' | 'success'>('loading');
  readonly message = signal('Completing sign-in...');

  constructor() {
    this.route.queryParamMap
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe((params) => {
        const code = params.get('code');
        if (!code) {
          this.status.set('error');
          this.message.set('Missing authorization code.');
          return;
        }
        this.completeAuth(code);
      });
  }

  private async completeAuth(code: string): Promise<void> {
    try {
      await this.auth.exchangeCode(code);
      this.status.set('success');
      this.message.set('Signed in. Redirecting to your dashboard...');
      await this.router.navigate(['/dashboard']);
    } catch {
      this.status.set('error');
      this.message.set('Sign-in failed. Please retry.');
    }
  }
}
