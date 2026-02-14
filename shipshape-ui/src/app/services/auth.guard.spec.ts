import { TestBed } from '@angular/core/testing';
import { Route, Router, UrlTree } from '@angular/router';

import { authGuard } from './auth.guard';
import { AuthService } from './auth.service';

describe('authGuard', () => {
  it('allows navigation when authenticated', async () => {
    const authStub = {
      ensureSession: jasmine.createSpy('ensureSession').and.resolveTo(true)
    };
    TestBed.configureTestingModule({
      providers: [
        { provide: AuthService, useValue: authStub },
        { provide: Router, useValue: { parseUrl: jasmine.createSpy('parseUrl') } }
      ]
    });

    const result = await TestBed.runInInjectionContext(() =>
      authGuard({} as Route, [])
    );

    expect(result).toBeTrue();
  });

  it('redirects to login when unauthenticated', async () => {
    const authStub = {
      ensureSession: jasmine.createSpy('ensureSession').and.resolveTo(false)
    };
    const urlTree = {} as UrlTree;
    const routerStub = {
      parseUrl: jasmine.createSpy('parseUrl').and.returnValue(urlTree)
    };
    TestBed.configureTestingModule({
      providers: [
        { provide: AuthService, useValue: authStub },
        { provide: Router, useValue: routerStub }
      ]
    });

    const result = await TestBed.runInInjectionContext(() =>
      authGuard({} as Route, [])
    );

    expect(result).toBe(urlTree);
    expect(routerStub.parseUrl).toHaveBeenCalledWith('/login');
  });
});
